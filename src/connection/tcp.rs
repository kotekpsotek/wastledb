use std::{io::{Write, Read}, fmt::format, vec};
#[path ="../login-system.rs"]
mod login_system;
#[path ="../management"]
mod management {
    pub mod main;
}

use {
    std::net::{ TcpStream, TcpListener },
    std::io::{ BufReader, BufRead },
    std::collections::HashMap,
    std::time::SystemTime,
    std::path::Path,
    std::sync::{Arc, Mutex}
};
use datafusion::parquet::data_type::AsBytes;
use generic_array::{ typenum::{ UInt, UTerm, B1, B0 }, GenericArray};
use login_system::authenticate_user;
use uuid::{Uuid, timestamp};
use tokio;
use crate::inter;
use management::main::Outcomes::*;             
use rsa::{self, RsaPrivateKey, RsaPublicKey, pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey, DecodeRsaPrivateKey, DecodeRsaPublicKey}, PublicKeyParts, PublicKey, PaddingScheme};
use rand::{self, Rng};
use aes_gcm::{
    Aes256Gcm, Nonce, aead::{Aead, KeyInit, OsRng}
};

// 
enum ResponseTypes {
    Success(bool), // 1. whether attach to success response message user sess id
    Error(ErrorResponseKinds)
}

impl ResponseTypes {
    /** 
     * Function to handle TCP server response by write message response bytes to TcpStream represented by "stream" param. In case when response can't be send error message about that will be printed in cli
     * When user would like have encrypted ongoing message it is performed by this function
    **/
    fn handle_response(&self, from_command: Option<CommandTypes>, stream: Option<TcpStream>, sessions: Option<&mut HashMap<String, String>>, session_id: Option<String>) {
        /// When user would like to encrypt message then encrypt or return message in raw format (that fact is inferred from SessionData struct by this function)
        fn response_message_generator(command_type: Option<CommandTypes>, sessions: Option<&mut HashMap<String, String>>, session_id: Option<String>, message: String) -> String {
            if let Some(sess) = sessions {
                if let Some(sess_id) = session_id {
                    let session = sess.get(&sess_id);
                    if session.is_some() {
                        let session_data = serde_json::from_str::<SessionData>(session.unwrap()).unwrap();
                        if let Some(encryption) = session_data.encryption {
                            // Encrypt message using previous generated encryption datas
                            let enc_message = CommmunicationEncryption::aes_256_gcm_encrypt(
                                &encryption.aes_gcm_key, 
                                CommmunicationEncryption::from_hex_to_vec_bytes(&encryption.nonce), 
                                message.as_bytes()
                            );
                            let enc_message_hex = CommmunicationEncryption::convert_vec_bytes_to_hex(enc_message);
                            
                            // Prepare encrypted message format
                                // Plaintext info required to decrypt message body
                            let info_to_decrypt = {
                                let mut inf = format!("nonce|x=x|{nonce}", nonce = encryption.nonce);
                                    // .. Aes key is attached only to register command and when command hasn't been entered. After when response has been achived by client Secret key is cached by client for whole ongoing connection 
                                    // When command hasn't been entered that means that use can hasn't got cached AES Secret key (in situation when he didn't achived "Register" command response)
                                if command_type.is_none() || (command_type.is_some() && command_type.unwrap() == CommandTypes::Register) {
                                    let aes_key = format!(" 1-1 aes|x=x|{aes_key_t}", aes_key_t = encryption.aes_gcm_key);
                                    inf.push_str(&aes_key);
                                }
                                inf
                            };
                                // Ciphertext with information to decrypt message body / When Err(()) from encryption function then return empty string which will be send to client
                            let ciphertext_info_message = CommmunicationEncryption::rsa_encrypt_message(info_to_decrypt)
                                .map_or_else(|_| String::new(), |enc_mes| enc_mes);

                            // Return formed message
                            format!("{key_and_nonce};{message_body}", key_and_nonce = ciphertext_info_message, message_body = enc_message_hex)
                        } 
                        else {
                            message
                        }
                    }
                    else {
                        message
                    }
                }
                else {
                    message
                }
            }
            else {
                message
            }
        }
        
        // Give appropriate action to determined response status
        let mut result_message: String = "NOT".to_string();
            // When below code not handle response type in that case "NOT" response is returned to client
        if matches!(self, ResponseTypes::Error(_)) { // handle error responses
            // handle not-sucesfull reasons (only here)
            if matches!(self, ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason)) || matches!(self, ResponseTypes::Error(ErrorResponseKinds::IncorrectRequest | ErrorResponseKinds::GivenSessionDoesntExists | ErrorResponseKinds::SessionTimeExpired)) { // Handle all for message "Err" response
                let message_type = "Err;";
                if matches!(self, ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason)) {
                    result_message = format!("{}{}", message_type, "UnexpectedReason");
                }
                else if matches!(self, ResponseTypes::Error(ErrorResponseKinds::GivenSessionDoesntExists)) { // given session by you doesn't exist
                    result_message = format!("{}{}", message_type, "SessionDoesntExists")
                }
                else if matches!(self, ResponseTypes::Error(ErrorResponseKinds::SessionTimeExpired)) { // session can't be extended
                    result_message = format!("{}{}", message_type, "SessionCouldntBeExtended")
                }
                else { // for all different
                    result_message = format!("{}{}", message_type, "IncorrectRequest");
                }
            }
            else if matches!(self, ResponseTypes::Error(ErrorResponseKinds::IncorrectLogin)) {
                result_message = String::from("IncLogin;Null")
            }
            else if matches!(self, ResponseTypes::Error(ErrorResponseKinds::CouldntPerformQuery(_))) { // when query couldn't be performed
                result_message = format!("Couldn't perform query") // TODO: add reason from "ErrorResponseKinds::CouldntPerformQuery(_)" (is in omitted place by "_")
            }
        }
        else if matches!(self, ResponseTypes::Success(_)) { // handle success responses
            result_message = {
                if matches!(self, ResponseTypes::Success(true)) {
                        //...here session id must be attached to method call
                    let session_id = session_id.clone().expect(&format!("You must attach session id to \"{}\" method in order to handle correct results when {}", stringify!(self.handle_response), stringify!(Self::Success(true))));
                    let respose_msg_raw =  format!("OK;{}", session_id).to_string();

                    // return "OK;session_id"
                    respose_msg_raw
                }
                else {
                    "OK".to_string()
                }
            }
        }

        // Send response
            // Below defined clousure is for print information that can't send response
        let couldnt_send_response = || {
            println!("Couldn't send response to client. Error durning try to create handler for \"TCP stream\"")
        };
        match stream {
            Some(mut stri) => {
                    // Encrypt response message when user would like to have it or send plaintext reponse for example when from some reason couldn't encrypt message
                let ready_result_message_raw = response_message_generator(from_command, sessions, session_id, result_message);
                    // Send to user encrypted response payload when all Option<_> arguments are enclosed in Some() variant and everything other is good (private and public keys are present, private key are in correct pkcs#1_pem format), [all RSA keys are stored in session data as pem strings]
                    // Try send to user response
                let resp = stri.write(ready_result_message_raw.as_bytes());
                if let Err(_) = resp { // time when to user can't be send response because error is initialized durning creation of http server
                    couldnt_send_response();
                }
                // else {
                //     println!("Response sended, {}", result_message)
                // }
            },
            None => { // time when to user can't be send response because error is initialized durning creation of http server
                couldnt_send_response();
                // println!("b")
            }
        }
    }
}

#[derive(Debug)]
enum ErrorResponseKinds {
    IncorrectRequest,
    UnexpectedReason,
    IncorrectLogin, // when user add incorrect login data or incorrect login data format (this difference is important)
    GivenSessionDoesntExists, // when session doesn't exists
    SessionTimeExpired, // when session time expired and couldn't live more
    CouldntPerformQuery(String) // send when couldnt perform query sended in command from some reason
}

#[derive(Debug, Clone, PartialEq)]
struct LoginCommandData { // setup connection data
    login: String,
    password: String,
    connected_to_db: Option<String>,
    use_rsa: bool
}

#[derive(Clone, Debug, PartialEq)]
enum CommandTypes {
    Register,
    Command,
    KeepAlive,
    RegisterRes(LoginCommandData), // Result of parsing "Register" command recognizer prior as "Register" child
    KeepAliveRes(String, u128), // 1. Is for id of session retrived from msg_body, 2. Is for parse KeepAlive result where "u128" is generated timestamp of parse generation
    CommandRes(String) // 1. SQL query content is attached under
}

#[derive(Debug)]
pub struct CommandTypeKeyDiff<'s> { 
    name: &'s str,
    value: &'s str
}

impl CommandTypes {
    // Parse datas from recived request command
    // Return command type and its data such as login data
    // SQL query are processed inside
    #[allow(unused_must_use)] // Err should be ignored only for inside call where are confidence of correcteness
    fn parse_cmd(&self, msg_body: &str, sessions: Option<&mut HashMap<String, String>>) -> Result<CommandTypes, ErrorResponseKinds> {
        if matches!(self, Self::Register) { // command to login user and setup connection
            if msg_body.len() > 0 {
                let msg_body_sep = msg_body.split(" 1-1 ").collect::<Vec<&str>>();
                if msg_body_sep.len() >= 2 { // isnide login section must be 2 pieces: 1 - login|x=x|logindata 2 - password|x=x|passworddata 3 - connect_auto|x=x|true
                    
                    let mut keys_required_list = LoginCommandData { 
                        login: String::new(), 
                        password: String::new() ,
                        connected_to_db: if msg_body_sep.len() >= 3 { // must be give as 3 body key
                            let val_option = self.clone().parse_key_value(msg_body_sep[2]);
                            // TODO: Order of optional params shouldn't have any matter like in "use_rsa" key
                            
                            if val_option.is_some() {
                                let val_option = val_option.unwrap();

                                if val_option.name == "connect_auto" && val_option.value.len() > 0 && Path::new(&format!("../source/dbs/{}", val_option.value)).exists() {
                                    let db_name = val_option.value.to_string();
                                    Some(db_name)
                                }
                                else {
                                    None
                                }
                                
                            }
                            else {
                                None
                            }
                        }
                        else {
                            None
                        },
                        use_rsa: if msg_body_sep.len() >= 3 { // whether communication between database and client should be encrypted using "RSA" algorithm
                            let mut whether_use = false;
                            for val_option in &msg_body_sep[1..] { // order of optional params shouldn't have any matter
                                let key_val = self.clone().parse_key_value(val_option);
                                if let Some(key_val) = key_val {
                                    if key_val.name == "rsa" { // when "rsa" option has been found
                                        if let Ok(val) = key_val.value.parse::<bool>() { // value presented for "rsa" should be boolean data-type
                                            whether_use = val
                                        };
                                        break;
                                    }
                                }
                            }
                            whether_use
                        }
                        else {
                            false
                        }
                    };

                    // Sepearte value from key and assing value to return struct
                    for key in msg_body_sep {
                        let key_separated = self.clone().parse_key_value(key);

                        if let Some(key) = key_separated { // when some that means key value or key name isn't empty (guaranted by "elf.parse_key_value(key)" function)
                            if key.name == "password" {
                                keys_required_list.password = key.value.to_string();
                            }
                            else if key.name == "login" {
                                keys_required_list.login = key.value.to_string();
                            }
                        }
                    }
                    
                    // When login data or password data isn't empty that means all is good
                    if keys_required_list.login.len() > 0 && keys_required_list.password.len() > 0 {
                        Ok(CommandTypes::RegisterRes(keys_required_list))
                    }
                    else {
                        Err(ErrorResponseKinds::IncorrectLogin)
                    }
                }
                else {
                    Err(ErrorResponseKinds::IncorrectRequest)
                }
            }
            else {
                Err(ErrorResponseKinds::IncorrectLogin)
            }
        }
        else if matches!(self, Self::KeepAlive) { // command for extend session life
            let sessions = sessions.expect("Sessions mustn't be None value");
            if sessions.contains_key(msg_body) {
                let ses_id = msg_body;

                    //...Get session data and parse it from json format
                let json_session_data = sessions.get(ses_id).unwrap();
                let session_data_struct = serde_json::from_str::<SessionData>(json_session_data).unwrap(); // we assume that in session storage are only correct values!

                    //...Retrive timestamp from session data and generate new timestamp to comparison
                let timestamp = session_data_struct.timestamp;
                let timestamp_new = get_timestamp();

                    //...Test Whether session expiration can be extended and extend when can be 
                if (timestamp + inter::MAXIMUM_SESSION_LIVE_TIME_MILS) >= timestamp_new {
                    Ok(CommandTypes::KeepAliveRes(ses_id.to_string(), timestamp_new))
                }
                else {
                    Err(ErrorResponseKinds::SessionTimeExpired)
                }
            }
            else {
                Err(ErrorResponseKinds::GivenSessionDoesntExists)
            }
        }
        else if matches!(self, Self::Command) { // command for execute sql query in database
            let msg_body_sep = msg_body.split(" 1-1 ").collect::<Vec<&str>>();
            if msg_body_sep.len() >= 2 { // isnide login section must be minimum 2 pieces: 1 - session_id|x=x|session_id 2 - sql_query|x=x|sql query which will be executed on db, 3 - ...described inside block
                //... session
                let request_session = self
                    .clone()
                    .parse_key_value(msg_body_sep[0]);

                if let Some(CommandTypeKeyDiff { name, value: session_id }) = request_session {
                    let sessions = sessions.unwrap(); 
                    if name == "session_id" && sessions.contains_key(session_id) { // key "session_id" must always be first
                        //... query
                        let sql_query_key = self
                            .clone()
                            .parse_key_value(msg_body_sep[1]);
                        // Using when user would like connect to database after create database using 'CREATE DATABASE database_name' SQL query // this option always must be 3 in order and value assigned to it must be "boolean"
                        let connect_auto = if msg_body_sep.len() >= 3 { // to pass this option must be presented prior 2 keys so: 1. session_id, 2. command (src)  
                            if let Some(CommandTypeKeyDiff { name, value }) = self.clone().parse_key_value(msg_body_sep[2]) {
                                if name == "connect_auto" && vec!["true", "false"].contains(&value) {
                                    Some(
                                        CommandTypeKeyDiff {
                                            name,
                                            value
                                        }
                                    )
                                }
                                else {
                                    None
                                }
                            }
                            else {
                                None
                            }
                        }
                        else {
                            None
                        };

                        // Process SQL query ... only when sql_query_key was correctly parsed prior
                        if let Some(CommandTypeKeyDiff { name, value }) = sql_query_key {
                            if name == "sql_query" && value.len() > 0 {
                                // Extend session live time after call this command (by emulate KeepAlive command manually)
                                CommandTypes::KeepAlive.parse_cmd(msg_body, Some(sessions));

                                // Process query + return processing result outside this method
                                let q_processed_r = self::management::main::process_query(value, connect_auto, session_id.into(), sessions);
                                match q_processed_r {
                                    Success(desc_opt) => {
                                        if let Some(desc) = desc_opt {
                                            return Ok(CommandTypes::CommandRes(desc))
                                        };

                                        // outcome when from processing sql query function has been returned Success(None) (without content description)
                                        Ok(CommandTypes::CommandRes(format!("Query has been performed")))
                                    },
                                    Error(reason) => Err(ErrorResponseKinds::CouldntPerformQuery(reason))
                                }
                            }
                            else {
                                Err(ErrorResponseKinds::IncorrectRequest)
                            }
                        }
                        else {
                            Err(ErrorResponseKinds::IncorrectRequest)
                        }
                    }
                    else {
                        Err(ErrorResponseKinds::IncorrectRequest)
                    }
                }
                else {
                    Err(ErrorResponseKinds::IncorrectRequest)
                }
            }
            else {
                Err(ErrorResponseKinds::IncorrectRequest)
            }
        }
        else { 
            Err(ErrorResponseKinds::UnexpectedReason)
        }
    }

    // Separate key from value
    // Return "None" when: added key size is empty or not contains separator, separated zones using key-value separator length is different than 2, key name size is empty or key value is empty
    fn parse_key_value(self, msg_body_key: &str) -> Option<CommandTypeKeyDiff> {
        if msg_body_key.len() > 0 && msg_body_key.contains("|x=x|") {
            let sep_key_zones = msg_body_key.split("|x=x|").collect::<Vec<&str>>();
            
            if sep_key_zones.len() == 2 && sep_key_zones[0].len() > 0 && sep_key_zones[1].len() > 0 {
                Some(CommandTypeKeyDiff {
                    name: sep_key_zones[0],
                    value: sep_key_zones[1]
                })
            }
            else {
                None
            }
        }
        else {
            None
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct SessionData {
    timestamp: u128,
    connected_to_database: Option<String>,
    encryption: Option<CommmunicationEncryption>
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct CommmunicationEncryption {
    aes_gcm_key: String,
    nonce: String
}

type AesKeyHexString = String;
type AesNonceHexString = String;
#[allow(dead_code)]
/// Modes available for generate rsa keys
enum GenRsaModes {
    /// Generate keys and return its both from generator function in PEM formats as string separate public and sep private
    Normal,
    /// Generate Keys in PEM format and save them into separate files in folder for KEYS (replacing old keys when it exists)
    SaveToFiles
}
#[allow(dead_code)]
/// Representation of both rsa keys
enum GetRsaKey {
    /// Public key
    Public,
    /// Private key
    Private
}
/// Valulable interface for ensure private, secure exchange data between client and SQL database
impl CommmunicationEncryption {
    const FOLDER_TO_RSA_KEYS: &str = "keys";

    /// Convert Vector<u8> (containing bytes) to hex string which values are separated by whitespace " "
    fn convert_vec_bytes_to_hex(vec_bytes: Vec<u8>) -> String {
        let mut result_str = Vec::new() as Vec<String>;
        for byte in vec_bytes {
            let str_r = format!("{:X}", byte);
            result_str.push(str_r);
        }

        result_str.join(" ")
    }

    /// Convert from Vec<String> (containing hexes) to Vector with u8. You should worry: because here aren't any ensures that byte is correct utf-8/ascii encoding byte like in AES-256-gcm storing format (always in hex)
    fn from_hex_to_vec_bytes(hex_string: &String) -> Vec<u8> {
        let hex_vec = hex_string.split(" ").map(|val| val.to_string()).collect::<Vec<String>>();
        let mut vec_u8: Vec<u8> = Vec::new();

        for hex_str in hex_vec {
            let u8_str = u8::from_str_radix(&hex_str, 16).expect("Coluldn't convert hex to byte again");
            vec_u8.push(u8_str);
        }

        vec_u8
    }
    
    /// Generate RSA keys (public, private) and return in PEM format
    fn gen_rsa_keys(mode: GenRsaModes) -> Result<(Option<String>, Option<String>), ()> {
        let rsa_key_len = 4096; // key length in bytes // 4096 is the most secure and appropriate key base on NIST recomendation
        
        // Gen. Private key and public key
        let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), rsa_key_len).unwrap();
        let public_key = RsaPublicKey::from(&private_key);
        
        // Convert keys to PEM format
        let string_private_key = RsaPrivateKey::to_pkcs1_pem(&private_key, rsa::pkcs1::LineEnding::CRLF).unwrap().to_string();
        let string_public_key = RsaPublicKey::to_pkcs1_pem(&public_key, rsa::pkcs1::LineEnding::CRLF).unwrap().to_string();

        // Return key in PEM format
        use self::GenRsaModes::*;
        match mode {
            Normal => {
                // in this mode both keys will be returned and not saved in any place
                Ok((Some(string_private_key), Some(string_public_key)))
            },
            SaveToFiles => {
                // One function to save "private" and "public" keys
                let save_key = |key_type: &str, key: String| {
                    if ["private", "public"].contains(&key_type) {
                        let p_str = format!("{}/{}.pem", Self::FOLDER_TO_RSA_KEYS, key_type);
                        let path = std::path::Path::new(&p_str);
                        return std::fs::write(path, key).map_err(|_| ())
                    }

                    Err(())
                };
                
                // save provate key
                save_key("private", string_private_key)?;

                //save public key
                save_key("public", string_public_key)?;

                // branchback result
                Ok((None, None))
            }
        }
    }

    /// Get specified (provate or public) RSA key from .pem file
    /// When you would like get Pru=ivate key you recive Ok((Some(private_key), None)) but when you would like public key you recive Ok((None, Some(public_key)))
    fn get_rsa_key(key_type: GetRsaKey) -> Result<(Option<RsaPrivateKey>, Option<RsaPublicKey>), ()> {
        //! public.pem and private.pem should be generated prior then this function was invoked. .pem files with keys are under folder name defined in "FOLDER_TO_RSA_KEYS" constant (this folder is in same location as "src" folder)
        use GetRsaKey::*;

        let obtain_key = || -> Result<String, ()> {
            let p_f = {
                let mut base = format!("{}/", Self::FOLDER_TO_RSA_KEYS);
                match key_type {
                    Private => base.push_str("private.pem"),
                    Public => base.push_str("public.pem")
                };
                base
            };
            let path_keys = std::path::Path::new(&p_f);
            let key = std::fs::read_to_string(path_keys).map_err(|_| ())?;
            Ok(key)
        };
        let key_raw = obtain_key()?;
        
        return match key_type {
            Private => RsaPrivateKey::from_pkcs1_pem(&key_raw).map_or_else(|_| Err(()), |key| Ok((Some(key), None))),
            Public => RsaPublicKey::from_pkcs1_pem(&key_raw).map_or_else(|_| Err(()), |key| Ok((None, Some(key))))
        };
    }

    /// Generate AES Nonce. Return: (string nonce as hex, nonce bytes in vector)
    fn gen_aes_nonce() -> (AesNonceHexString, Vec<u8>) {
        // Generate nonce and convert it to hex
        let nonce_slice = vec![0; 12];
        let nonce: &GenericArray<u8, UInt<UInt<UInt<UInt<UTerm, B1>, B1>, B0>, B0>> = &Nonce::from_slice(&nonce_slice[..]); // 256-bits; unique per message (means: created in first and re-generated in each next new response)
        let string_hex_nonce = Self::convert_vec_bytes_to_hex(nonce.to_vec());

        // branchback
        (string_hex_nonce, nonce_slice) // 1 - nonce in hex string format, 2 - nonce in bytes
    }

    /// Generate AES Secret key
    fn gen_aes_key() -> AesKeyHexString {
        // Generate AES key and nonce
        let aes_key = Aes256Gcm::generate_key(&mut OsRng);
        
        // Convert aes key hex
        let string_hex_aes_key = Self::convert_vec_bytes_to_hex(aes_key.to_vec()); // obtain Vec<u8> (Vector with bytes) to hex format then create from outcoming Vec String by separate hex values using "whitespace character"

        // branchback
        string_hex_aes_key
    }

    /// Obtain AES key from previous saved hex string
    fn aes_obtain_key(key_str_hex: &String) -> GenericArray::<u8, UInt<UInt<UInt<UInt<UInt<UInt<UTerm, B1>, B0>, B0>, B0>, B0>, B0>> {
        let key_vec = Self::from_hex_to_vec_bytes(key_str_hex);
        let ready_encrypt_key = GenericArray::<u8, UInt<UInt<UInt<UInt<UInt<UInt<UTerm, B1>, B0>, B0>, B0>, B0>, B0>>::from_slice(&key_vec[..]);
        
        // Aes key
        ready_encrypt_key.to_owned()
    }

    /// Encrypt message using AES-256-GCM. Return: 1 - Ciphertext in bytes form (not correct on utf-8 sake)
    fn aes_256_gcm_encrypt(key_str_hex: &String, nonce: Vec<u8>, msg: &[u8]) -> Vec<u8> {
        // Obtain previous generated AES key
        let ready_encrypt_key = Self::aes_obtain_key(key_str_hex);

        // Encrypt message using key and nonce
        let cipher = Aes256Gcm::new(&ready_encrypt_key);
        let nonce = Nonce::from_slice(&nonce[..]); // nonce must have got 32 length
        let ciphertext = cipher.encrypt(nonce, msg).unwrap(); // in encryption process we have more control on what is encryptes thus .unwrap() in this scenarion isn't such bad

        // Prepare returned values
        ciphertext
    }

    /// Decrypt message encrypted using AES-256-GCM
    fn aes_256_gcm_decrypt(key_str_hex: &String, nonce: Vec<u8>, msg_hex: &String) -> Result<Vec<u8>, ()> {
        // Obtain previous generated AES key
        let ready_encrypt_key = Self::aes_obtain_key(key_str_hex);

        // Convert message to decrypt from Hex format
        let message_vec_bytes = Self::from_hex_to_vec_bytes(msg_hex);

        // Decrypt message using key and nonce
        let cipher = Aes256Gcm::new(&ready_encrypt_key);
        let nonce = Nonce::from_slice(&nonce[..]);
        let plaintext = cipher.decrypt(nonce,&message_vec_bytes[..]).map_err(|_| ())?;

        // Return encrypted key when it can be encrypted
        Ok(plaintext)
    }

    /// Encrypt message using RSA private key (designed for encrypt Secret Key and Nonce) (max length of encrypted message = mod from rsa key size)
    /// Return when: Ok(_) = encrypted message in hex format (where hex values are sepparated using whitespace " "), Err(_) - "()" = "tuple type" without elements inside 
    fn rsa_encrypt_message(msg: String) -> Result<String, ()> {
        let private_key = Self::get_rsa_key(GetRsaKey::Private)?.0
            .map_or_else(|| Err(()), |key| Ok(key))?;
        let encrypted_mess = private_key.encrypt(&mut rand::thread_rng(), PaddingScheme::new_pkcs1v15_encrypt(), msg.as_bytes())
            .map_err(|_| ())?;
        let encrypted_mess_hex = Self::convert_vec_bytes_to_hex(encrypted_mess);
        
        // Return encrypted message as hex
        Ok(encrypted_mess_hex)
    }

    /// Decrypt message using RSA public key (message to decrypt must be encrypted using RSA private key from same pair otherwise Err(() will be returned))
    /// Return: when Ok(_) = decrypted message in valid utf-8 string, Err(_) - "()" = "tuple type" without elements inside 
    fn rsa_decrypt_message(msg_hex: String) -> Result<String, ()> {
        let public_key = Self::get_rsa_key(GetRsaKey::Private)?.0
            .map_or_else(|| Err(()), |key| Ok(key))?;
        let encrypted_mess_bytes = Self::from_hex_to_vec_bytes(&msg_hex);
        let decrypted_mess = public_key.decrypt(PaddingScheme::new_pkcs1v15_encrypt(), &encrypted_mess_bytes[..])
            .map_or_else(|_| Err(()), |enc| {
                // Try convert decrypted message to UTF-8 / When couldn't then return Err(()) in order to propagate him outside clausure 
                String::from_utf8(enc)
                    .map_or_else(|_| Err(()), |dec_stri| Ok(dec_stri))
            })?;
        
        // When not error catched above then return decrypted message as Vaalid UTF-8 characters
        Ok(decrypted_mess)
    }
}

// Callculate timestamp (how much milliseconds flow from 1 January 1970 to function invoke time)
fn get_timestamp() -> u128 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => dur.as_millis(),
        Err(_) => 0
    }
}

// Call as 2
// Handle pending request and return request message when it is correct
// Err -> when: couldn't read request, colund't convert request to utf-8 string
fn handle_request(stream: &mut TcpStream) -> Result<String, ()> {
    // Recive Request
    let mut req_buf = [0; inter::MAXIMUM_REQUEST_SIZE_BYTES];
    match stream.read(&mut req_buf) {
        Ok(_size) => {
            // Create String from response
            let mut intermediate_b = Vec::<u8>::new();
            
                //... add to "intermediate" bytes other then "0" 
            for byte in req_buf {
                if byte == 0 {
                    break;
                };
                intermediate_b.push(byte);
            };

                //... convert "intermediate" to String and return it or error
            if let Ok(c_request) = String::from_utf8(intermediate_b) {
                Ok(c_request)
            }
            else {
                return Err(())
            }
        },
        Err(_) => {
            Err(())
        }
    }
}

// "Call as 2"
// Recoginize commands and parse it then return Ok() when both steps was berformed correctly or return Err() when these both steps couldn't be performed. Error is returned as ErrorResponseKinds enum which can be handled directly by put it into enum "ResponseTypes" and call to method ".handle_response(tcp_stream)"
fn process_request(c_req: String, sessions: Option<&mut HashMap<String, String>>) -> Result<CommandTypes, ErrorResponseKinds> {
    let message_semi_spli = c_req.split(";").collect::<Vec<&str>>(); // split message using semicolon
    if message_semi_spli.len() > 1 { // must be at least 2 pieces: "Message Type" and second in LTF order "Message Body"
            // mes type
        let message_type = message_semi_spli[0].to_lowercase();
        let message_type = message_type.as_str();
            // mes body
        let message_body = message_semi_spli[1];
       
        // Handle command
        if message_type == "command" { // Execute command into db here
            CommandTypes::Command.parse_cmd(message_body, sessions)
        }
        else if message_type == "register" { // login user into database and save his session
            CommandTypes::Register.parse_cmd(message_body, None) // When Ok(_) is returned: CommandTypes::RegisterRes(LoginCommandData { login: String::new("login datas"), password: String::new("password datas") })
        }
        else if message_type == "keep-alive" { // keep user session saved when
            CommandTypes::KeepAlive.parse_cmd(message_body, sessions) // Process message body and return
        }
        else { // when unsuported message was sended
            Err(ErrorResponseKinds::IncorrectRequest)
        }
    }
    else { // when after separation message by its parts exists less or equal to 1 part in Vector
        Err(ErrorResponseKinds::IncorrectRequest)
    }
}

// "Call from outside to connect all chunks together"
pub async fn handle_tcp() {
    let tcp_server_adress = format!("0.0.0.0:{port}", port = inter::TCP_PORT);
    let listener = TcpListener::bind(tcp_server_adress).expect("Couldn't spawn TCP Server on selected port!");
    let mut sessions = Arc::new(Mutex::new(HashMap::<String, String>::new())); // key - session id, data - session data in json format

    // Sessions interval
    tokio::spawn({
        let ses = Arc::clone(&mut sessions);
        async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await; // execute for every 10 seconds
                let mut lc = ses.lock().unwrap();

                // iterate over sessions timestamps
                for entry in lc.clone().iter() {
                    let s_d = serde_json::from_str::<SessionData>(entry.1).unwrap();
                    let s_d_ct = s_d.timestamp;
                    let timestamp_new = get_timestamp();

                    if timestamp_new - s_d_ct > inter::MAXIMUM_SESSION_LIVE_TIME_MILS {
                        lc.remove(entry.0);
                    }
                }
            }
        }
    });

    // Tcp request
    for request in listener.incoming() {
        if let Ok(mut stream) = request {
            match handle_request(&mut stream) {
                Ok(c_req) => {
                    let mut sessions = sessions.lock().unwrap();
                    
                    /* Do more... */
                    match process_request(c_req.clone(), Some(&mut sessions)) {
                        Ok(command_type) => {
                            match command_type {
                                // Save user session
                                CommandTypes::RegisterRes(LoginCommandData { login, password, connected_to_db, use_rsa }) => {
                                    // ...Check login corecteness
                                    let s = Some(stream);
                                    if authenticate_user(login, password) {
                                        fn gen_uuid(sessions: &HashMap<String, String>) -> String {
                                            let uuid_v = Uuid::new_v4().to_string();
                                            if sessions.contains_key(&uuid_v) {
                                               return gen_uuid(sessions);
                                            };
                                            uuid_v
                                        }
                                        
                                        // Save session into sessios list
                                            //... Generate uuid_v4
                                        let uuid_gen = gen_uuid(&sessions);
                                            //... Compose session data in form of struct
                                        let session_data = SessionData {
                                            timestamp: get_timestamp(),
                                            connected_to_database: connected_to_db,
                                            encryption: {
                                                if use_rsa {
                                                    Some(CommmunicationEncryption {
                                                        nonce: CommmunicationEncryption::gen_aes_nonce().0,
                                                        aes_gcm_key: CommmunicationEncryption::gen_aes_key()
                                                    })
                                                }
                                                else {
                                                    None
                                                }
                                            }
                                        };

                                            //... Serialize session data into JSON, handle result and send appropriate response to what is Result<..> outcome
                                        match serde_json::to_string(&session_data) {
                                            Ok(ses_val) => {
                                                // Add session value to sessions list
                                                sessions.insert(uuid_gen.clone(), ses_val);
                                                // println!("{:?}", sessions); // Test: print all sessions in list after add new session

                                                // Send response
                                                    // Attach public key to response for bring encryption between client and server. Only attached when "use_rsa" option has been attached to request
                                                ResponseTypes::Success(true).handle_response(Some(CommandTypes::Register), s, Some(&mut *sessions), Some(uuid_gen))
                                            },
                                            _ => ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason).handle_response( Some(CommandTypes::Register), s, None, None)
                                        };
                                    }
                                    else {
                                        ResponseTypes::Error(ErrorResponseKinds::IncorrectLogin).handle_response(Some(CommandTypes::Register), s, None, None)
                                    }
                                },
                                CommandTypes::KeepAliveRes(ses_id, timestamp) => { // command to extend session life (heartbeat system -> so keep-alive)
                                    // extend session to new timestamp
                                        //...Parse extended session timestamp to other not changed session data
                                    let old_session_data = sessions.get(&ses_id).unwrap(); // assumes that in this place session must exists and is able to be extended
                                    let new_sess_data = SessionData {
                                        timestamp,
                                        ..serde_json::from_str::<SessionData>(old_session_data).unwrap()
                                    };
                                    // println!("New: {:#?}\n\nOld: {:#?}", new_sess_data, old_session_data); //Test: integrity check log
                                    let json_new_sess_data = serde_json::to_string(&new_sess_data).unwrap();
                                        //...Update session
                                    sessions.insert(ses_id.clone(), json_new_sess_data);

                                        // Send response
                                    ResponseTypes::Success(false).handle_response(Some(CommandTypes::KeepAlive), Some(stream), Some(&mut *sessions), Some(ses_id))
                                },
                                CommandTypes::CommandRes(query) => {
                                    // Furthermore process query by database
                                    println!("Query: {}", query);

                                    // Send success response to client
                                    ResponseTypes::Success(false).handle_response(Some(CommandTypes::Command), Some(stream), Some(&mut *sessions), None)
                                }
                                _ => () // other types aren't results
                            }
                        },
                        Err(err_kind) => ResponseTypes::Error(err_kind).handle_response(None, Some(stream), None, None)
                    };
                }
                Err(_) => {
                    /* handle probably error */
                    println!("Recived response is incorrect!");
                    if let Err(_) = stream.shutdown(std::net::Shutdown::Both) {
                        println!("Couldn't close TCP connection after handled incorrect response!");
                    }
                }
            }
        }
        else { // while error durning creation of stream handler
            ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason).handle_response(None, None, None, None)
        }
    }
}


#[cfg(all(test))]
mod tests {
    use std::borrow::Borrow;

    use rsa::{PublicKey, PaddingScheme};

    use super::CommmunicationEncryption;

    /* #[test]
    fn ks() {
        use aes_gcm::{
            aead::{Aead, KeyInit, OsRng},
            Aes256Gcm, Nonce // Or `Aes128Gcm`
        };
        
        let key = Aes256Gcm::generate_key(&mut OsRng);
        let cipher = Aes256Gcm::new(&key);
        let nonce_slice = vec![0; 12];
        let nonce = Nonce::from_slice(nonce_slice.as_bytes()); // 96-bits; unique per message
        let ciphertext = cipher.encrypt(nonce, b"plaintext message".as_ref()).unwrap();
        let plaintext = cipher.decrypt(nonce, ciphertext.as_ref()).unwrap();
    } */

    #[test]
    fn enc_keys() {
        let aes_key = CommmunicationEncryption::gen_aes_key();
        let aes_nonce = CommmunicationEncryption::gen_aes_nonce(); // 1. Nonce hex string, 2. Nonce bytes Vector

        // Below: Convert aes_key stored as String to Vec<u8> only that convertion allow to create same AesKey to in encode and decode messages by one
        let aes_key_bytes = CommmunicationEncryption::from_hex_to_vec_bytes(aes_key.borrow());
        println!("Aes key in u8 after decryption: {:?}", aes_key_bytes);

        // Below: Complete process End-To-End Encrypt message using pre-generated keys and decrypt it again
        let encryption_result = CommmunicationEncryption::aes_256_gcm_encrypt(aes_key.borrow(), aes_nonce.1.clone(), b"message to decrypt");
        println!("Encryption result (in bytes): {:?}", encryption_result);
        let encrypted_message_hex = CommmunicationEncryption::convert_vec_bytes_to_hex(encryption_result);
        println!("Convert encrypted message result to hex (in hex): {}", encrypted_message_hex);
        let decryption_results = CommmunicationEncryption::aes_256_gcm_decrypt(&aes_key, aes_nonce.1, &encrypted_message_hex).expect("Message couldn't been decrypted!");
        println!("Result of decryption of message (in bytes): {:?}", decryption_results);
        let again_to_string = String::from_utf8(decryption_results).expect("Couldn't convert encrypted message to utf-8 string (rather some byte isn't correct with utf-8 characters bytes)");
        println!("Decrypted message as string (utf-8 string): {}", again_to_string)
    }

    #[test]
    /// Test/Generate RSA key-pair to .pem files located into "./keys" folder (in same location as "src" folder is nested)
    fn save_rsa_pem_files() {
        let save_files_result = CommmunicationEncryption::gen_rsa_keys(super::GenRsaModes::SaveToFiles).expect("Couldn't save files with RSA PEM keys");
        println!("{:?}", save_files_result);
    }

    #[test]
    /// Generate RSA keys-pair without save it to some file
    fn generate_rsa_in_pem() {
        let genrate_rsa_result = CommmunicationEncryption::gen_rsa_keys(super::GenRsaModes::Normal).expect("Couldn't generate RSA keys-pair"); // if no panic then 2 rsa keys in 2 Some variants should be returned in tuple like (private keys, public key) 
        println!("{:?}", genrate_rsa_result);
    }

    #[test]
    fn encrypt_rsa() {
        let encrypted_message = CommmunicationEncryption::rsa_encrypt_message("Encrypted message using RSA".to_string()).expect("Couldn't encrypt message using RSA algo");
        println!("{}", encrypted_message)
    }

    #[test]
    fn decrypt_rsa() {
        let priv_key = CommmunicationEncryption::get_rsa_key(super::GetRsaKey::Private).unwrap().0.unwrap();
        let mess_encrypted_privatek = priv_key.encrypt(&mut rand::thread_rng(), PaddingScheme::new_pkcs1v15_encrypt(), b"message encrypted using rsa").expect("Couldn't encrypt message using private RSA key");
        let decrypt_message = CommmunicationEncryption::rsa_decrypt_message(CommmunicationEncryption::convert_vec_bytes_to_hex(mess_encrypted_privatek.clone())).expect("Colund't decrypt message using RSA public key");
        println!("Decrypted message: {}, Previous encrypted message: {:?}", decrypt_message, mess_encrypted_privatek);
    }

    #[test]
    /// Works same as "register_user_by_tcp" but it is furthermore to decrypt response when previous user suggest option for "Register" Command as rsa=true
    // Should be defined in "main.rs", but it is here because majority imports from test is defined in this file
    fn register_and_decrypt() {
        // obtain response message
        let mut response = crate::tests::register_user_by_tcp();

        // Replace "blank characters" from response to not obtain panics (while convertion from hex is processing)
        response = response.replace("\0", "");

        // split response message to two fragments
        let data_split = response.split(";").collect::<Vec<_>>();

        // obtain two response fragments (both are represented as HEX)
        let data_to_decrypt_hex = data_split[0].to_string();
        let data_message_hex = data_split[1].to_string();

        // Convert message fragments from slice to vectors with message bytes
        let data_to_decrypt = CommmunicationEncryption::from_hex_to_vec_bytes(&data_to_decrypt_hex);
        // let data_message = CommmunicationEncryption::from_hex_to_vec_bytes(&data_message_hex); // unncecessary

        // Decrypt "data_to_decrypt" fragment using RSA private key
        let priv_key = CommmunicationEncryption::get_rsa_key(super::GetRsaKey::Private)
            .expect("Couldn't get RSA private key")
            .0.expect("Inside private key field is none");
        let data_to_decrypt_decoded = priv_key.decrypt(super::PaddingScheme::new_pkcs1v15_encrypt(), &data_to_decrypt[..]).expect(r#"Couldn't decrypt "data to decrypt message body" using RSA Private Key"#);
        
        // Convert data to decrypt message to string in aim to allow decode response body and process it further
        let data_to_decrypt_string = String::from_utf8(data_to_decrypt_decoded).expect(&format!("Couldn't convert {} to UTF-8 string", stringify!(data_to_decrypt)));

        // Obtain from "data_to_decrpt" fargment valulable informations
        let fragments = data_to_decrypt_string.split(" 1-1 ").collect::<Vec<_>>();
            // Nonce data
        let nonce_key_data = fragments[0];
        let nonce_split = nonce_key_data.split("|x=x|").collect::<Vec<_>>();
        let nonce_data_hex = nonce_split[1];
        let nonce_data_bytes = CommmunicationEncryption::from_hex_to_vec_bytes(&nonce_data_hex.to_string());
            // AES data
        let aes_key_data = fragments[1];
        let aes_key_data_split = aes_key_data.split("|x=x|").collect::<Vec<_>>();
        let aes_key_data_data_hex = aes_key_data_split[1];

        // Decrypt message body
        let data_message_body_bytes = CommmunicationEncryption::aes_256_gcm_decrypt(aes_key_data_data_hex.to_string().borrow(), nonce_data_bytes, &data_message_hex).expect("Couldn't decode message body"); // converts directly message in hex format so manually (outside the function) isn't necessary to convert message with hex to bytes 

        // Convert message body to string format
        let data_message_string = String::from_utf8(data_message_body_bytes).expect(&format!("Couldn't convert {} to UTF-8 string", stringify!(data_message)));
    
        // Show results as whole operation finall result
        println!("Datas to decrypt message are: {}\nMessage body is: {}", data_to_decrypt_string, data_message_string)
    }
}
