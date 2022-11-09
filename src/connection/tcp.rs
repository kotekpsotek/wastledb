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
use rsa::{self, RsaPrivateKey, RsaPublicKey, pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey, DecodeRsaPrivateKey}, PublicKeyParts, PublicKey, PaddingScheme};
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
     *  "public_rsa_key" param - is attached to response only in case when user send to server request with picked option "rsa=true" and only in communication initialization command so for "Regiseter" only
    **/
    fn handle_response(&self, stream: Option<TcpStream>, sessions: Option<&mut HashMap<String, String>>, session_id: Option<String>, public_rsa_key: Option<String>) {
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
                    let mut respose_msg =  format!("OK;{}", session_id).to_string();

                        // Add public key to response (Attached when use choose earlier rsa communication encryption)
                    if let Some(public_rsa_key) = public_rsa_key {
                        respose_msg.push_str(&format!("|x=x|{}", public_rsa_key));
                    };
                    
                    // WARNING: Session id and whole payload isn't encrypted using RSA private key
                    respose_msg
                }
                else {
                    "OK".to_string()
                }
            }
        }

        // Send response
            // or inform that response can't be send
        let couldnt_send_response = || {
            println!("Couldn't send response to client. Error durning try to create handler for \"TCP stream\"")
        };
        match stream {
            Some(mut stri) => {
                    // When encryption result is Some() then send to user encrypted payload using RSA Private Key otherwise send to user raw response payload
                let ready_result_message_raw = result_message.as_bytes();
                    // Send to user encrypted response payload when all Option<_> arguments are enclosed in Some() variant and everything other is good (private and public keys are present, private key are in correct pkcs#1_pem format), [all RSA keys are stored in session data as pem strings]
                let response_ready = match CommmunicationEncryption::encrypt_response(session_id.clone(), sessions, ready_result_message_raw) { 
                    Some(b_msg) => b_msg,
                    None => ready_result_message_raw.to_vec()
                };
                    // Try send to user response
                let resp = stri.write(&response_ready[..]);
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

#[derive(Debug, Clone)]
struct LoginCommandData { // setup connection data
    login: String,
    password: String,
    connected_to_db: Option<String>,
    use_rsa: bool
}

#[derive(Clone, Debug)]
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
    private_key: String,
    public_key: String,
    aes_gcm_key: String,
    nonce: String
}

/// Valulable interface for ensure private, secure exchange data between client and SQL database
impl CommmunicationEncryption {
    /// Convert Vector<u8> (containing bytes) to vector with hex values
    fn convert_vec_bytes_to_hex(vec_bytes: Vec<u8>) -> Vec<String> {
        let mut result_str = Vec::new() as Vec<String>;
        for byte in vec_bytes {
            let str_r = format!("{:X}", byte);
            result_str.push(str_r);
        }

        result_str
    }

    /// Convert from Vec<String> (containing hexes) to Vector with u8. You should worry: because here aren't any ensures that byte is correct utf-8/ascii encoding byte like in AES-256-gcm storing format (always in hex)
    fn from_hex_to_vec_bytes(vec_hexes: Vec<String>) -> Vec<u8> {
        let mut vec_u8: Vec<u8> = Vec::new();

        for hex_str in vec_hexes {
            let u8_str = u8::from_str_radix(&hex_str, 16).expect("Coluldn't convert hex to byte again");
            vec_u8.push(u8_str);
        }

        vec_u8
    }
    
    /// Generate all required keys to establish and perform secure communication in both ways
    // INFORMATION: Generation RSA keys (2: 1 - public, 1 - private) takes some additional time // To speedup keys generation you should comply to adivice given on "https://github.com/RustCrypto/RSA" ("rsa" crate authors github repo) -> you should add optimalizations to Cargo.toml file to speedup generation of the key (global optymalizations or for essential crate what is "num-bigint-dig" (add opt-level) to this crate should speedup keys generation time  20 times almost or more (I didn't measure it with timer))
    fn gen_keys() -> Self {
        let key_len = 4096; // key length in bytes // 4096 is the most secure and appropriate key base on NIST recomendation
        let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), key_len).unwrap();
        let public_key = RsaPublicKey::from(&private_key);
        let aes_key = Aes256Gcm::generate_key(&mut OsRng);
        let nonce_slice = vec![0; 12];
        let nonce: &GenericArray::<u8, UInt<UInt<UInt<UInt<UInt<UInt<UTerm, B1>, B0>, B0>, B0>, B0>, B0>> = Nonce::from_slice(nonce_slice.as_bytes()); // 96-bits; unique per message (means: created in first and re-generated in each next new response)
        let string_private_key = RsaPrivateKey::to_pkcs1_pem(&private_key, rsa::pkcs1::LineEnding::CRLF).unwrap().to_string();
        let string_public_key = RsaPublicKey::to_pkcs1_pem(&public_key, rsa::pkcs1::LineEnding::CRLF).unwrap().to_string();
        let string_hex_aes_key = Self::convert_vec_bytes_to_hex(aes_key.to_vec()).join(" "); // obtain Vec<u8> (Vector with bytes) to hex format then create from outcoming Vec String by separate hex values using "whitespace character"
        let string_hex_nonce = Self::convert_vec_bytes_to_hex(nonce.to_vec()).join(" ");

        // this object will be store into sessions
        Self {
            private_key: string_private_key,
            public_key: string_public_key,
            aes_gcm_key: string_hex_aes_key,
            nonce: string_hex_nonce // here nonce is initially generated
        }
    }

    /// Function to facilitate obtaining AES key for encryption and decryption processes
    fn aes_obtain_key(key_str_hex: &String) -> GenericArray::<u8, UInt<UInt<UInt<UInt<UInt<UInt<UTerm, B1>, B0>, B0>, B0>, B0>, B0>> {
        let key_vec = Self::from_hex_to_vec_bytes(key_str_hex.split(" ").collect::<Vec<&str>>().iter().map(|val| val.to_string()).collect::<Vec<String>>());
        let ready_encrypt_key = GenericArray::<u8, UInt<UInt<UInt<UInt<UInt<UInt<UTerm, B1>, B0>, B0>, B0>, B0>, B0>>::from_slice(&key_vec[..]);
        
        ready_encrypt_key.to_owned()
    }

    /// Encrypt given message using for that previous generated aes-gcm key (key is generated and attached to session when first communication was recived from client)
    fn aes_256_gcm_encrypt(key_str_hex: &String, msg: &[u8]) -> (Vec<u8>, Vec<u8>) {
        // Obtain previous generated AES key
        let ready_encrypt_key = Self::aes_obtain_key(key_str_hex);

        // Encrypt message using key and nonce
        let cipher = Aes256Gcm::new(&ready_encrypt_key);
        let nonce_slice = vec![0; 12];
        let nonce = Nonce::from_slice(nonce_slice.as_bytes()); // 96-bits; unique per message (means: created in first and re-generated in each next new response)
        let ciphertext = cipher.encrypt(nonce, msg).unwrap(); // in encryption process we have more control on what is encryptes thus .unwrap() in this scenarion isn't such bad

        // Prepare returned values
        (ciphertext, nonce_slice)
    }

    /// Decrypt recived message as hex string to form of Vector with probably correct utf-8 bytes. 
    fn aes_256_gcm_decrypt(key_str_hex: &String, nonce: Vec<u8>, msg_hex: &String) -> Result<Vec<u8>, ()> {
        // Obtain previous generated AES key
        let ready_encrypt_key = Self::aes_obtain_key(key_str_hex);

        // Decrypt message using key and nonce
        let cipher = Aes256Gcm::new(&ready_encrypt_key);
        let message_vec_bytes = Self::from_hex_to_vec_bytes(msg_hex.split(" ").collect::<Vec<&str>>().iter().map(|val| val.to_string()).collect::<Vec<String>>());
        // TODO: Same action with nonce
        let nonce = Nonce::from_slice(&nonce[..]);
        let plaintext = cipher.decrypt(nonce,&message_vec_bytes[..]).map_err(|_| ())?;

        // Return encrypted key when it can be encrypted
        Ok(plaintext)
    }

    /// Encrypt response payload using for this generated previous private key from RSA keys pair (private key, public key) and AES Secret (AES-256-GCM)
    /// Nonce (for block ciher) will be new for each dbs response
    /// All params enclosed in Option<_> must be Some() variant in order to encrypt message!
    fn encrypt_response<'l>(session_id: Option<String>, sessions: Option<&mut HashMap<String, String>>, result_message_raw: &[u8]) -> Option<Vec<u8>> {
            // Try to obtain session_id String
        let sess_id = if let Some(sess_id) = session_id {
            Some(sess_id)
        }
        else {
            None
        }?;
            // Perform only when sess_id = Some(session_id) and sessions = Some(sessions_list). Get user session data which are im json format (user is represented by "session_id" param)
        let user_sess_dat = if let Some(ses) = sessions {
            ses.get(&sess_id)
        }
        else {
            None
        }?;
            // Obtain user data from session
        let user_sess_dat_struct = serde_json::from_str::<SessionData>(user_sess_dat)
            .unwrap();
        let user_sess_encryption_dat = user_sess_dat_struct.encryption?;

        // Cipher message using AES-256-GCM
        let encrypted_message_op = Self::aes_256_gcm_encrypt(&user_sess_encryption_dat.aes_gcm_key, result_message_raw);
        let encrypted_message = encrypted_message_op.0;
        let nonce = encrypted_message_op.1;

        // Encrypt AES key using RSA
            // Extract user sess dat as sessions struct. Session data always should be in correct json format
        // let user_private_key = serde_json::from_str::<SessionData>(user_sess_dat)
        //     .unwrap()
        //     .encryption 
        //     .map_or_else(|| None, |enc| {
        //         Some(enc.private_key)
        //     })?;
        //     // Format "user_private_key" string to private key struct
        // let private_key_encr_ready = RsaPrivateKey::from_pkcs1_pem(&user_private_key)
        //     .map_or_else(|_| None, |key| Some(key))?;
        //     // Prepare required params to encrypt message using private key and encrypt message + convert encryption result which is Vec<u8> to &[u8]
        // let mut rng = rand::thread_rng();
        // let padding = PaddingScheme::new_pkcs1v15_encrypt();
        // println!("{:?}", result_message_raw);
        // let encrypted_message = private_key_encr_ready.encrypt(&mut rng, padding, result_message_raw)
        //     .map_or_else(|err| {panic!("{}", err); None}, |enc_mess| Some(enc_mess))?;

            // Return success response
        // Some(encr_message)       
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
                                                    Some(CommmunicationEncryption::gen_keys()) // generate both required keys for rsa // WARNING: this always take some additional time
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
                                                let public_key = {
                                                    if let Some(keys_pack) = session_data.encryption {
                                                        Some(keys_pack.public_key)
                                                    } 
                                                    else {
                                                        None
                                                    }
                                                };
                                                ResponseTypes::Success(true).handle_response(s, Some(&mut *sessions), Some(uuid_gen), public_key)
                                            },
                                            _ => ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason).handle_response(s, None, None, None)
                                        };
                                    }
                                    else {
                                        ResponseTypes::Error(ErrorResponseKinds::IncorrectLogin).handle_response(s, None, None, None)
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
                                    ResponseTypes::Success(false).handle_response(Some(stream), Some(&mut *sessions), Some(ses_id), None)
                                },
                                CommandTypes::CommandRes(query) => {
                                    // Furthermore process query by database
                                    println!("Query: {}", query);

                                    // Send success response to client
                                    ResponseTypes::Success(false).handle_response(Some(stream), Some(&mut *sessions), None, None)
                                }
                                _ => () // other types aren't results
                            }
                        },
                        Err(err_kind) => ResponseTypes::Error(err_kind).handle_response(Some(stream), None, None, None)
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
        let keys_set = CommmunicationEncryption::gen_keys();

        // Below: Convert aes_key stored as String to Vec<u8> only that convertion allow to create same AesKey to in encode and decode messages by one
        // let decrypted_aes_key = CommmunicationEncryption::from_hex_to_vec_bytes(keys_set.aes_gcm_key.split(" ").collect::<Vec<&str>>().iter().map(|val| val.to_string()).collect::<Vec<String>>());
        // println!("Aes key in u8 after decryption: {:?}", decrypted_aes_key)

        // Below: Complete process End-To-End Encrypt message using pre-generated keys and decrypt it again
        let encryption_results = CommmunicationEncryption::aes_256_gcm_encrypt(&keys_set.aes_gcm_key, b"message which will be encrypted");
        println!("Encryption result (in bytes): {:?}", encryption_results);
        let encrypted_message_hex = CommmunicationEncryption::convert_vec_bytes_to_hex(encryption_results.0).join(" ");
        println!("Convert encrypted message result to hex (in hex): {}", encrypted_message_hex);
        let decryption_results = CommmunicationEncryption::aes_256_gcm_decrypt(&keys_set.aes_gcm_key, encryption_results.1, &encrypted_message_hex).expect("Message couldn't been decrypted!");
        println!("Result of decryption of message (in bytes): {:?}", decryption_results);
        let again_to_string = String::from_utf8(decryption_results).expect("Couldn't convert encrypted message to utf-8 string (rather some byte isn't correct with utf-8 characters bytes)");
        println!("Decrypted message as string (utf-8 string): {}", again_to_string)
    }
}
