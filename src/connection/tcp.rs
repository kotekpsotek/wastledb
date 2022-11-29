use std::{io::{Write, Read}, vec};
#[path ="../login-system.rs"]
mod login_system;
#[path ="../management"]
mod management {
    pub mod main;
}

use {
    std::net::{ TcpStream, TcpListener },
    std::collections::HashMap,
    std::time::SystemTime,
    std::path::Path,
    std::sync::{Arc, Mutex},
    std::fs,
    std::str
};
use generic_array::{ typenum::{ UInt, UTerm, B1, B0 }, GenericArray};
use login_system::authenticate_user;
use uuid::Uuid;
use tokio;
use crate::inter;
use serde_json::json; // json macro to create JSON object
use management::main::Outcomes::*;             
use rsa::{self, RsaPrivateKey, RsaPublicKey, pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey, DecodeRsaPrivateKey, DecodeRsaPublicKey}, PublicKey, PaddingScheme};
use rand;
use aes_gcm::{
    Aes256Gcm, Nonce, aead::{Aead, KeyInit, OsRng}
};

// List of DBS response types
enum ResponseTypes {
    Success(bool), // 1. whether attach to success response message user sess id
    Error(ErrorResponseKinds)
}

// Handle response in staright forward way
impl ResponseTypes {
    /** 
     * Function to handle TCP server response by write message response bytes to TcpStream represented by "stream" param. In case when response can't be send error message about that will be printed in cli
     * When user would like have encrypted ongoing message it is performed by this function
    **/
    fn handle_response(&self, from_command: Option<CommandTypes>, stream: Option<&TcpStream>, sessions: Option<&mut HashMap<String, String>>, session_id: Option<String>, response_content: Option<String>) {
        /// When user would like to encrypt message then encrypt or return message in raw format (that fact is inferred from SessionData struct by this function)
        fn response_message_generator(command_type: Option<CommandTypes>, sessions: Option<&mut HashMap<String, String>>, session_id: Option<String>, message: String) -> String {
            if sessions.is_some() && session_id.is_some() {
                let sessions = sessions.unwrap();
                let session_id = session_id.unwrap();
                
                if let Some(session) = sessions.get(&session_id) {
                    if let Some(encryption) = serde_json::from_str::<SessionData>(session).unwrap().encryption {
                        if command_type.is_some() && matches!(command_type.as_ref().unwrap(), CommandTypes::InitializeEncryptionRes) {
                            // When command type is InitializeEncryption then encrypt data to decrypt message using only dbs rsa private key
                            let message_enc_bytes = CommmunicationEncryption::rsa_encrypt_message(message).unwrap();
                            
                            // Code message to hex format
                            ConnectionCodec::code_encrypted_message(message_enc_bytes)
                        }
                        else {
                            // Encrypt normal message using previous generated encryption datas
                            let enc_message = CommmunicationEncryption::aes_256_gcm_encrypt(
                                &encryption.aes_gcm_key, 
                                ConnectionCodec::decode_encrypted_message(encryption.nonce.clone()).expect("Couldn't decode nonce string to vec with bytes"),
                                message.as_bytes()
                            );
                            let enc_message = ConnectionCodec::code_encrypted_message(enc_message); // code encrypted using AES message

                            // Return encrypted message under HEX strings under which are not valid utf-8 characters
                            enc_message
                        }
                    }
                    else {
                        // Send message without any encryption but encoded as hex
                        ConnectionCodec::code_hex(message)
                    }
                }
                else {
                    // Send message without any encryption but encoded as hex
                    ConnectionCodec::code_hex(message)
                }
            }
            else {
                // Send message without any encryption but encoded as hex
                ConnectionCodec::code_hex(message)
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
                // Add reason or assign default reason
                result_message = match self {
                    ResponseTypes::Error(ErrorResponseKinds::CouldntPerformQuery(reason)) => {
                        format!("Err;{}", reason.to_owned())
                    },
                    _ => format!("Err;Couldn't perform query")
                }
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
                    let mut res = format!("OK");
                    // When response content has been passed to params (as "Some(content_string)") then send response with this content
                    match response_content {
                        Some(content) => {
                            res.push_str(&format!(";{}", content));
                            ()
                        },
                        None => ()
                    };
                    res
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
                // Get ecrypted message and encoded to correct hex format
                let ready_result_message_raw = response_message_generator(from_command, sessions, session_id, result_message);
                // Send response when it is possible as hex codes under which can be ciphertext or plaintext depends on what user would like to have
                let resp = stri.write(ready_result_message_raw.as_bytes());
                // Put appropriate action when response couldn't been send (prepared to send cuz RAII)
                if let Err(_) = resp { // time when to user can't be send response because error is initialized durning creation of http server
                    couldnt_send_response();
                }
            },
            None => { // time when to user can't be send response because error is initialized durning creation of http server
                couldnt_send_response();
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
    connected_to_db: Option<String>
}

#[derive(Clone, Debug, PartialEq)]
/// Supported commnad types and command datas
enum CommandTypes {
    Register,
    Command,
    KeepAlive,
    Show,
    DatabaseConnect, // change database from which user is connected
    InitializeEncryptionRes, // returned after detection "initializeencryptuon" command without any message body processing
    RegisterRes(LoginCommandData), // Result of parsing "Register" command recognizer prior as "Register" child
    KeepAliveRes(Option<String>, u128), // 1. Is for id of session retrived from msg_body / None (when connection is encrypted because session id in that time is returned in tuple), 2. Is for parse KeepAlive result where "u128" is generated timestamp of parse generation
    CommandRes(String), // 1. SQL query content is attached under
    ShowRes(String), // Outcome to show into String type
    DatabaseConnectRes(String, Option<String>) // 1. Database name, 2. Session ID / None (when connection is encrypted because session id in that time is returned in tuple)
}
// Distinguish command and return deserialized data from it
impl CommandTypes {
    // Parse datas from recived request command
    // Return command type and its data such as login data
    // SQL query are processed inside
    #[allow(unused_must_use)] // Err should be ignored only for inside call where are confidence of correcteness
    fn parse_cmd(&self, msg_body: &str, sessions: Option<&mut HashMap<String, String>>, connection_encrypted: bool, additional_data: Option<&String>) -> Result<CommandTypes, ErrorResponseKinds> {
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
            if !connection_encrypted {
                // Parse only for not encrypted connection
                if sessions.contains_key(msg_body) {
                    let ses_id = msg_body.to_string();
    
                        //...Get session data and parse it from json format
                    let json_session_data = sessions.get(&ses_id).unwrap();
                    let session_data_struct = serde_json::from_str::<SessionData>(json_session_data).unwrap(); // we assume that in session storage are only correct values!
    
                        //...Retrive timestamp from session data and generate new timestamp to comparison
                    let timestamp = session_data_struct.timestamp;
                    let timestamp_new = get_timestamp();
    
                        //...Test Whether session expiration can be extended and extend when can be 
                    if (timestamp + inter::MAXIMUM_SESSION_LIVE_TIME_MILS) >= timestamp_new {
                        let ses_id_res = if ses_id.len() != 0 {
                                Some(ses_id)
                            }
                            else {
                                None
                            };
                        Ok(CommandTypes::KeepAliveRes(ses_id_res, timestamp_new))
                    }
                    else {
                        Err(ErrorResponseKinds::SessionTimeExpired)
                    }
                }
                else {
                    Err(ErrorResponseKinds::GivenSessionDoesntExists)
                }
            }
            else {
                // When connection is encrypted then session id isn't in body and hence it isn't parsed here
                Ok(CommandTypes::KeepAliveRes(None, get_timestamp()))
            }
        }
        else if matches!(self, Self::Command) { // command for execute sql query in database
            let msg_body_sep = msg_body.split(" 1-1 ").collect::<Vec<&str>>();
            if msg_body_sep.len() >= 1 { // isnide login section must be minimum 2 pieces: 1 - sql_query|x=x|sql query which will be executed on db, 2 - ...described inside block, 3 - session_id|x=x|session_id
                // Action to perform
                let src_action = |sessions: &mut HashMap<String, String>, session_id: String| {
                    //... query
                    let sql_query_key = self
                        .clone()
                        .parse_key_value(msg_body_sep[0]);
                    // Using when user would like connect to database after create database using 'CREATE DATABASE database_name' SQL query // this option always must be 3 in order and value assigned to it must be "boolean"
                    let connect_auto = 
                        if msg_body_sep.len() >= 3 { // to pass this option must be presented prior 2 keys so: 1. session_id, 2. command (src)  
                            if let Some(CommandTypeKeyDiff { name, value }) = self.clone().parse_key_value(msg_body_sep[1]) {
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
                            CommandTypes::KeepAlive.parse_cmd(msg_body, Some(sessions), connection_encrypted, None);

                            // Process query + return processing result outside this method
                            let q_processed_r = self::management::main::process_query(value, connect_auto, session_id, sessions);
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
                };

                // Perform specific action
                let sessions = sessions.unwrap();
                if !connection_encrypted {
                    // When connection isn't encrypted
                    //... session
                    let request_session = self
                        .clone()
                        .parse_key_value(msg_body_sep.last().unwrap());

                    if let Some(CommandTypeKeyDiff { name, value: session_id }) = request_session {
                        if name == "session_id" && sessions.contains_key(session_id) {
                            src_action(sessions, session_id.to_string())
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
                    // When connection is encrypted
                    if let Some(session_id) = additional_data {
                        src_action(sessions, session_id.into())
                    }
                    else {
                        Err(ErrorResponseKinds::UnexpectedReason)
                    }
                }
            }
            else {
                Err(ErrorResponseKinds::IncorrectRequest)
            }
        }
        else if matches!(self, Self::Show) { // display tables list on database or specific table content
            let msg_body_sep = msg_body.split(" 1-1 ").collect::<Vec<&str>>();
            if msg_body_sep.len() >= 2 { // msg body must include 2 keys in same order as entered here: 1. session_id|x=x|sessionID 2. what|x=x|database_tables/table 3. unit_name|x=x|database_name/table_name (session id doesn't exists in message body when communication is encrypted but it is attached from outside to this function "additional_data" param)
                let what_key_val = self.clone().parse_key_value(msg_body_sep[0]);
                let what_unit = self.clone().parse_key_value(msg_body_sep[1]);
                let session_key = {
                    if msg_body_sep.len() > 2 {
                        self.clone().parse_key_value(msg_body_sep[2])
                    }
                    else {
                        None
                    }
                };

                // Check whether session was attached and prior initialized
                let src_action = |session_raw_content: &str| {
                    if what_key_val.is_some() && what_unit.is_some() {
                        let what_key_val = what_key_val.unwrap();
                        let what_unit = what_unit.unwrap();

                        // Clousure to check whether user is connected to database and that database name
                        let user_conn_t_db = || {
                            let session_dat_des = serde_json::from_str::<SessionData>(session_raw_content).unwrap();
                            match session_dat_des.connected_to_database {
                                Some(db_name) => {
                                    (true, db_name)
                                },
                                None => (false, String::new())
                            }
                        };

                        if what_key_val.name == "what" && what_unit.name == "unit_name" {
                            match what_key_val.value {
                                "database_tables" => {
                                    // User must be connected to specific database prior
                                    let chckdb = user_conn_t_db();
                                    if chckdb.0 {
                                        // Show all database tables
                                        let path_str = format!("../source/dbs/{db}", db = chckdb.1);
                                        let path = Path::new(&path_str);
                                        
                                        if path.exists() {
                                            let mut table_names = vec![] as Vec<String>; // only correct tables names without .json extension
                                            for entry in fs::read_dir(path).unwrap() {
                                                let entry = entry.unwrap().path();

                                                if entry.is_file() {
                                                    let et_n_s = entry.file_name().unwrap().to_str().unwrap().split(".").collect::<Vec<_>>();

                                                    if et_n_s.len() > 0 && *et_n_s.last().unwrap() == "json" {
                                                        table_names.push(et_n_s[..(et_n_s.len() - 1)].join(".")) // add table name without .json extension
                                                    }
                                                }
                                            };
                                        
                                            Ok(
                                                CommandTypes::ShowRes(
                                                    json!({
                                                        "tables": table_names
                                                    })
                                                    .to_string()
                                                )    
                                            )
                                        }
                                        else {
                                            Err(ErrorResponseKinds::CouldntPerformQuery("Entered database doesn't exists".to_string()))
                                        }
                                    }
                                    else {
                                        Err(ErrorResponseKinds::CouldntPerformQuery("To perform that command you must be firstly connected to database from which you'd like to obtain tables".to_string()))
                                    }
                                },
                                "table_records" => {
                                    // User must be connected to specific database prior
                                    let chckdb = user_conn_t_db();
                                    if chckdb.0 {
                                        // Show table (with columns including their names, datatypes and constraint and also table all records)
                                        let path_str = format!("../source/dbs/{db}/{tb}.json", db = chckdb.1, tb = what_unit.value);
                                        let path = Path::new(&path_str);
                                        
                                        if path.exists() {
                                            let table = fs::read_to_string(path).unwrap();

                                            if table.len() > 0 {
                                                Ok(
                                                    CommandTypes::ShowRes(
                                                        json!({
                                                            "table": table
                                                        })
                                                        .to_string()
                                                    )
                                                )
                                            }
                                            else {
                                                Err(ErrorResponseKinds::CouldntPerformQuery("Table is empty file!".to_string()))
                                            }
                                        }
                                        else {
                                            Err(ErrorResponseKinds::CouldntPerformQuery("Entered table name doesn't exists in database to which you're connected".to_string()))
                                        }
                                    }
                                    else {
                                        Err(ErrorResponseKinds::CouldntPerformQuery("To perform that command you must be firstly connected to database from which you'd like to obtain table data".to_string()))
                                    }
                                },
                                "databases" => {
                                    let path_str = format!("../source/dbs");
                                    let path = Path::new(&path_str);
    
                                    if path.exists() {
                                        let mut databases_names: Vec<String> = vec![];
                                        for entry in fs::read_dir(path).unwrap() {
                                            let entry = entry.unwrap().path();
                                            if entry.is_dir() {
                                                databases_names.push(entry.file_name().unwrap().to_str().unwrap().to_string())
                                            }
                                        };
    
                                        Ok(
                                            CommandTypes::ShowRes(
                                                json!({
                                                    "databases": databases_names
                                                })
                                                .to_string()
                                            )
                                        )
                                    }
                                    else {
                                        Err(ErrorResponseKinds::UnexpectedReason)
                                    }
                                },
                                _ => {
                                    Err(ErrorResponseKinds::IncorrectRequest)
                                }
                            }
                        }
                        else {
                            Err(ErrorResponseKinds::IncorrectRequest)
                        }
                    }
                    else {
                        Err(ErrorResponseKinds::IncorrectRequest)
                    }
                };

                // Specific bucket for specific communication
                if !connection_encrypted {
                    // when connection isn't encrypted
                    if session_key.is_some() {
                        let session_key = session_key.unwrap();
                        if session_key.name == "session_id" {
                            if let Some(session_storage) = sessions {
                                // Extend session live time after call this command (by emulate KeepAlive command manually)
                                CommandTypes::KeepAlive.parse_cmd(msg_body, Some(session_storage), connection_encrypted, None);
    
                                // Src action
                                if let Some(session) = session_storage.get(session_key.value) {
                                    // Compute command body
                                    src_action(&session)
                                }
                                else {
                                    Err(ErrorResponseKinds::GivenSessionDoesntExists)
                                }
                            }
                            else {
                                Err(ErrorResponseKinds::UnexpectedReason)
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
                    // when connection is encrypted
                    src_action(additional_data.unwrap())
                }
            }
            else {
                Err(ErrorResponseKinds::IncorrectRequest)
            }
        }
        else if matches!(self, Self::DatabaseConnect) { // connect user with specific database name // user must be singin with database prior
            let msb_sp = msg_body.split(" 1-1 ").collect::<Vec<_>>();
            if msb_sp.len() > 0 {
                let db_name = self.clone().parse_key_value(msb_sp[0]);
                let session_id = {
                    if msb_sp.len() > 1 && !connection_encrypted {
                        self.clone().parse_key_value(msb_sp[1])
                    }
                    else {
                        None
                    }
                };
                
                if db_name.is_some() && connection_encrypted {
                    // For not encrypted connection
                    let db_name = db_name.unwrap();
                    let session_id = session_id.unwrap();
                    // Check de-parsed keys names correcteness
                    if session_id.name == "session_id" && db_name.name == "database_name" {
                        // Check whether session exists
                        if let Some(_) = sessions.as_ref().unwrap().get(session_id.value) {
                            // Extend session live time after call this command (by emulate KeepAlive command manually)
                            CommandTypes::KeepAlive.parse_cmd(msg_body, Some(sessions.unwrap()), connection_encrypted, None);

                            Ok(CommandTypes::DatabaseConnectRes(db_name.value.to_string(), Some(session_id.value.to_string())))
                        }
                        else {
                            Err(ErrorResponseKinds::GivenSessionDoesntExists)
                        }
                    }
                    else {
                        Err(ErrorResponseKinds::IncorrectRequest)
                    }
                }
                else if db_name.is_some() && !connection_encrypted {
                    let db_name = db_name.unwrap();
                    let session_id = session_id.unwrap();
                    if db_name.name == "database_name" && session_id.name == "session_id" && db_name.value.len() > 0 && session_id.value.len() > 0 {
                        // Extend session live time after call this command (by emulate KeepAlive command manually)
                        CommandTypes::KeepAlive.parse_cmd(msg_body, Some(sessions.unwrap()), connection_encrypted, None);

                        Ok(CommandTypes::DatabaseConnectRes(db_name.value.to_string(), Some(session_id.value.to_string())))
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

#[derive(Debug)]
pub struct CommandTypeKeyDiff<'s> { 
    name: &'s str,
    value: &'s str
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct SessionData {
    timestamp: u128,
    connected_to_database: Option<String>,
    encryption: Option<CommmunicationEncryption>
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct CommmunicationEncryption {
    /// AES key in hex string
    aes_gcm_key: String,
    /// AES NONCE in hex string
    nonce: String
}
/// Valulable interface for ensure private, secure exchange data between client and SQL database
impl CommmunicationEncryption {
    const FOLDER_TO_RSA_KEYS: &str = "keys";
    
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
        let string_hex_nonce = ConnectionCodec::code_encrypted_message(nonce.to_vec()); // hex codes without gaps between each (not valid utf-8)

        // branchback
        (string_hex_nonce, nonce_slice) // 1 - nonce in hex string format, 2 - nonce in bytes
    }

    /// Generate AES Secret key
    fn gen_aes_key() -> AesKeyHexString {
        // Generate AES key and nonce
        let aes_key = Aes256Gcm::generate_key(&mut OsRng);
        
        // Convert aes key hex
        let string_hex_aes_key = ConnectionCodec::code_encrypted_message(aes_key.to_vec()); // outcome hex codes without gaps between each others (not valid utf-8)

        // branchback
        string_hex_aes_key
    }

    /// Obtain AES key from previous saved hex string
    fn aes_obtain_key(key_str_hex: &String) -> GenericArray::<u8, UInt<UInt<UInt<UInt<UInt<UInt<UTerm, B1>, B0>, B0>, B0>, B0>, B0>> {
        let key_vec = ConnectionCodec::decode_encrypted_message(key_str_hex.clone()).expect("Couldn't obtain bytes vector from aes key in hex format");
        let ready_encrypt_key = GenericArray::<u8, UInt<UInt<UInt<UInt<UInt<UInt<UTerm, B1>, B0>, B0>, B0>, B0>, B0>>::from_slice(&key_vec[..]);
        
        // Aes key
        ready_encrypt_key.to_owned()
    }

    /// Encrypt message using AES-256-GCM. Return Ciphertext in bytes placed into Vector (Remember that under this vector isn't valid utf-8 characters required via Rust default encoding for String types)
    pub fn aes_256_gcm_encrypt(key_str_hex: &String, nonce: Vec<u8>, msg: &[u8]) -> Vec<u8> {
        // Obtain previous generated AES key
        let ready_encrypt_key = Self::aes_obtain_key(key_str_hex);

        // Encrypt message using key and nonce
        let cipher = Aes256Gcm::new(&ready_encrypt_key);
        let nonce = Nonce::from_slice(&nonce[..]); // nonce must have got 32 length
        let ciphertext = cipher.encrypt(nonce, msg).unwrap(); // in encryption process we have more control on what is encryptes thus .unwrap() in this scenarion isn't such bad

        // Prepare returned values
        ciphertext
    }

    /// Decrypt message encrypted using AES-256-GCM. Message to decrypt must be in vector with bytes obtained after decoding encrypted message. Return Vector with valid UTF-8 bytes
    pub fn aes_256_gcm_decrypt(key_str_hex: &String, nonce: Vec<u8>, msg: Vec<u8>) -> Result<Vec<u8>, ()> {
        // Obtain previous generated AES key
        let ready_encrypt_key = Self::aes_obtain_key(key_str_hex);

        // Decrypt message using key and nonce
        let cipher = Aes256Gcm::new(&ready_encrypt_key);
        let nonce = Nonce::from_slice(&nonce[..]);
        let plaintext = cipher.decrypt(nonce,&msg[..]).map_err(|_| ())?;

        // Return encrypted key when it can be encrypted
        Ok(plaintext)
    }

    /// Encrypt message using RSA private key (designed for encrypt Secret Key and Nonce) (max length of encrypted message = mod from rsa key size)
    /// Return when: Ok(_) = encrypted message as bytes in vectror (where hex values are sepparated using whitespace " "), Err(_) - "()" = "tuple type" without elements inside 
    fn rsa_encrypt_message(msg: String) -> Result<Vec<u8>, ()> {
        let private_key = Self::get_rsa_key(GetRsaKey::Private)?.0
            .map_or_else(|| Err(()), |key| Ok(key))?;
        let encrypted_mess_bytes = private_key.encrypt(&mut rand::thread_rng(), PaddingScheme::new_pkcs1v15_encrypt(), msg.as_bytes())
            .map_err(|_| ())?;
        
        // Return encrypted message as bytes
        Ok(encrypted_mess_bytes)
    }

    /// Decrypt message using RSA public key (message to decrypt must be encrypted using RSA private key from same pair otherwise Err(() will be returned))
    /// Return: when Ok(_) = decrypted message in valid utf-8 string, Err(_) - "()" = "tuple type" without elements inside 
    pub fn rsa_decrypt_message(msg_hex: String) -> Result<String, ()> {
        let public_key = Self::get_rsa_key(GetRsaKey::Private)?.0
            .map_or_else(|| Err(()), |key| Ok(key))?;
        let encrypted_mess_bytes = ConnectionCodec::decode_encrypted_message(msg_hex).expect("Couldn't convert message from hex string to bytes vec");
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

/// Code and decode connection. From HEX to string UTF-8 and in invert
/// Decoded String to be properly decoded must has got 2 characters
/// Situation that some charcater after encode/decode has got/should has got more then 2 characters is probable
pub struct ConnectionCodec;
impl ConnectionCodec {
    /// Code String message to hex format
    pub fn code_hex(message: String) -> String {
        let message = message.as_bytes();
        let mut hexes = vec![] as Vec<String>;
        
        for byte in message {
            hexes.push(format!("{:X}", byte)) // each hex code has got this form: 1F (each letter has got minimum number of characters in hex so 2) (For most case code builded from 2 characters is sufficient)
        }

        hexes.join("")
    }

    /// Encode encrypted message to string with hexes (Warning: Under each hex code isn't valid utf-8 character but ciphertext character)
    pub fn code_encrypted_message(message: Vec<u8>) -> String {
        let mut hexes = vec![] as Vec<String>;
        for byte in message {
            hexes.push(format!("{:03X}", byte)) // difference regard to function "code_hex" each code consist from 3 chaarcters and when it is smaller then 3 characters then always bengins with 0 (empty character)
        }

        hexes.join("")
    }

    /// Decode String message from hex format to utf-8
    pub fn decode_hex(message: String) -> Result<String, ()> {
        // After operation from 1 character HEX code you can create UTF-8 character (for clarity one UTF-8 character after encoding to HEX should create HEX code consists from 2 characters i.e: 2F)
        let splitted = message.as_bytes().chunks(2).map(str::from_utf8).collect::<Result<Vec<&str>, _>>().map_err(|_| ())?;
        let mut decoded_bytes = Vec::new() as Vec<u8>;

        // Iterate over each character presented under HEX code in 2 charcters i.e: 2F (UTF-8 character is: \)
        for splitted_char in splitted {
            if splitted_char != "\0\0" {
                let byte_utf8 = u8::from_str_radix(splitted_char, 16).map_err(|_| ())?; // when error return it directly from loop
                decoded_bytes.push(byte_utf8);
            }
            else {
                break;
            }
        }

        // Return utf-8 string created from HEX by that whole callable unit or Err
        String::from_utf8(decoded_bytes).map_err(|_| ())
    }

    /// Decode ciphertext as hexes string to ciphertext bytes
    pub fn decode_encrypted_message(message: String) -> Result<Vec<u8>, ()> {
        // Each encrypted message code consists from 3 charcters (different then normal coding) (character which has got smaller code then 3 characters starts with HEX character 0 = Null to real code (different then 0))
        let splitted = message.as_bytes().chunks(3).map(str::from_utf8).collect::<Result<Vec<&str>, _>>().map_err(|_| ())?;
        let mut decoded_bytes = Vec::new() as Vec<u8>;

        // Iterate over each character presented under HEX code in 2 charcters i.e: 2F (UTF-8 character is: \)
        for splitted_char in splitted {
            let byte_utf8 = u8::from_str_radix(splitted_char, 16).map_err(|_| ())?; // when error return it directly from loop
            decoded_bytes.push(byte_utf8);
        }

        // Return decoded bytes
        Ok(decoded_bytes)
    }
}

// Callculate timestamp (how much milliseconds flow from 1 January 1970 to function invoke time)
fn get_timestamp() -> u128 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => dur.as_millis(),
        Err(_) => 0
    }
}

// "Call as 3"
// Recoginize commands and parse it then return Ok() when both steps was berformed correctly or return Err() when these both steps couldn't be performed. Error is returned as ErrorResponseKinds enum which can be handled directly by put it into enum "ResponseTypes" and call to method ".handle_response(tcp_stream)"
fn process_request(c_req: String, sessions: Option<&mut HashMap<String, String>>) -> (Option<String>, Result<CommandTypes, ErrorResponseKinds>) {
    let message_semi_spli = (c_req).split(";").collect::<Vec<&str>>(); // split message using semicolon
    if message_semi_spli.len() > 1 { // must be at least 2 pieces: "Message Type" and second in LTF order "Message Body"
            // Session id exists only in this form when user established encrypted connection (When message is encrypted payload always consists from 3 pieces: Command, AES Encrypted and Encoded to not valid utf-8 HEX string, Session ID)
        let session_id = if message_semi_spli.len() == 3 {
                Some(message_semi_spli[2].to_string()) // Session id is always last
            }
            else {
                None
            };
            // It will say whether communication is encrypted
        let communication_is_encrypted_ind = session_id.as_ref().map_or_else(|| false, |sid| {
            if let Some(sessions) = &sessions {
                if sessions.contains_key(sid) {
                    return serde_json::from_str::<SessionData>(sessions.get(sid).unwrap()).unwrap().encryption.is_some();
                }
            };
            
            false
        });
            // mes type
        let message_type = message_semi_spli[0].to_lowercase();
        let message_type = message_type.as_str();
            // mes body // When encryption was established decrypt it (message body) here
        let message_body = {
            let msb = match session_id.as_ref() {
                Some(sid) => { // To perform decryption message must be in form specific for encryption
                    // Try to obtain data to decrypt mes body and decrypt it after
                    if let Some(sessions) = &sessions { // sessions must be attached to function invoke
                        if let Some(ses_dat) = sessions.get(&sid.clone()) {
                            let ses_dat = serde_json::from_str::<SessionData>(ses_dat).unwrap();
                            if let Some(enc) = ses_dat.encryption {
                                let nonce = ConnectionCodec::decode_encrypted_message(enc.nonce).unwrap();
                                let aes_key = enc.aes_gcm_key;
                                let message_to_decrypt_hex = message_semi_spli[1]; // Encrypted message is encoded to not-valid HEX so first must be decoded from that format to ciphertext (not valid utf-8 form)
                                let message_to_decrypt_decoded = ConnectionCodec::decode_encrypted_message(message_to_decrypt_hex.to_string()).expect("Couldn't decode message");

                                // Decrypt ciphertext to plaintext
                                let dec_bytes = CommmunicationEncryption::aes_256_gcm_decrypt(&aes_key, nonce, message_to_decrypt_decoded).expect("Couldn't decrypt message");

                                // Create UTF-8 string from decrypted plaintext bytes vector
                                String::from_utf8(dec_bytes).unwrap()
                            }
                            else {
                                message_semi_spli[1].to_string()
                            }
                        }
                        else {
                            message_semi_spli[1].to_string()
                        }
                    }
                    else {
                        message_semi_spli[1].to_string()
                    }
                },
                None => message_semi_spli[1].to_string()
            };
            // Convert to &str type
            &msb.clone()[..]
        };

        // Handle command
        if message_type == "command" { // Execute command into db here
            (session_id, CommandTypes::Command.parse_cmd(message_body, sessions, communication_is_encrypted_ind, None))
        }
        else if message_type == "initializeencryption" {
            (session_id, Ok(CommandTypes::InitializeEncryptionRes)) // return directly without any processing because it isn't required
        }
        else if message_type == "register" { // login user into database and save his session
            (session_id, CommandTypes::Register.parse_cmd(message_body, None, communication_is_encrypted_ind, None)) // When Ok(_) is returned: CommandTypes::RegisterRes(LoginCommandData { login: String::new("login datas"), password: String::new("password datas") })
        }
        else if message_type == "keep-alive" { // keep user session saved when
            (session_id, CommandTypes::KeepAlive.parse_cmd(message_body, sessions, communication_is_encrypted_ind, None)) // Process message body and return
        }
        else if message_type == "show" { // display tables list on database or specific table content
            (session_id.clone(), CommandTypes::Show.parse_cmd(message_body, sessions, communication_is_encrypted_ind, session_id.as_ref()))
        }
        else if message_type == "databaseconnect" { // connect user with specific database name
            (session_id, CommandTypes::DatabaseConnect.parse_cmd(message_body, sessions, communication_is_encrypted_ind, None))
        }
        else { // when unsuported message was sended
            (session_id, Err(ErrorResponseKinds::IncorrectRequest))
        }
    }
    else { // when after separation message by its parts exists less or equal to 1 part in Vector
        (None, Err(ErrorResponseKinds::IncorrectRequest))
    }
}

// Call as 2
// Handle pending request and return request message when it is correct
// Err -> when: couldn't read request, colund't convert request to utf-8 string, couldn't decode hex codes to utf-8 character
fn handle_request(stream: &mut TcpStream) -> Result<String, ()> {
    // Recive Request
    let mut req_buf = [0; inter::MAXIMUM_REQUEST_SIZE_BYTES];
    // Read stream content and write it to intermediate buffer
    stream.read(&mut req_buf).map_err(|_| ())?;
    
    // Operation will creating String from request bytes
    let mut intermediate_b = Vec::<u8>::new();
    
    // Add to "intermediate" buffer all bytes different then "0" (null byte)
    for byte in req_buf {
        if byte == 0 {
            break;
        };
        intermediate_b.push(byte);
    };

    // Convert to not separated HEX codes string
    let hex_cstring_request = String::from_utf8(intermediate_b).map_err(|_| ())?;
        
    // Create from not-separated hex codes string, valid utf-8 string or propagate error
    let decoded_letters = ConnectionCodec::decode_hex(hex_cstring_request)?;

    // Return UTF-8 response
    Ok(decoded_letters)
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
                    
                    /*  */
                    let sc = sessions.clone(); // sessions 
                    let pr = process_request(c_req.clone(), Some(&mut sessions));
                    // Check whether recived session id is encrypted and whether it is correct when encrypted connection was established
                    let check_sid_u_enc = |sessions: &HashMap<String, String>| {
                        if let Some(sid) = &pr.0 {
                            if sessions.contains_key(sid) {
                                return serde_json::from_str::<SessionData>(sessions.get(sid).unwrap()).unwrap().encryption.is_some()
                            };
                        };

                        false
                    };

                    // Handling commands
                    match pr.1 {
                        Ok(command_type) => {
                            match command_type {
                                // Create encryption between server and client
                                CommandTypes::InitializeEncryptionRes => {
                                    // That command create encrypted session and likely new session so it must be called firstly than other commands and among others "Register" command
                                        // Data required to create encrypted communications for next commands
                                    let aes_key = CommmunicationEncryption::gen_aes_key();
                                    let (aes_nonce_string, _) = CommmunicationEncryption::gen_aes_nonce();
                                    
                                        // Create session with specjalized datas for encryption
                                    let session_id = uuid::Uuid::new_v4().to_string();
                                    let encrypted_sdat = SessionData {
                                        timestamp: get_timestamp(),
                                        connected_to_database: None,
                                        encryption: Some(
                                            CommmunicationEncryption { aes_gcm_key: aes_key.to_owned(), nonce: aes_nonce_string.to_owned() }
                                        )
                                    };
                                    let encrypted_sdat = serde_json::to_string(&encrypted_sdat).unwrap();
                                    sessions.insert(session_id.to_owned(), encrypted_sdat);

                                        // Send response to client // response_cnt will be encrypted using RSA private key
                                    let response_cnt = format!("aes|x=x|{aesk} 1-1 nonce|x=x|{nonce} 1-1 session_id|x=x|{sid}", aesk = aes_key, nonce = aes_nonce_string, sid = session_id);
                                    ResponseTypes::Success(false).handle_response(Some(CommandTypes::InitializeEncryptionRes), Some(&stream), Some(&mut *sessions), Some(session_id), Some(response_cnt))
                                },
                                // Save user session
                                CommandTypes::RegisterRes(LoginCommandData { login, password, connected_to_db }) => {
                                    // update session data and send response
                                    let mut update_session_and_res = |sid: &String, sdata| match serde_json::to_string::<SessionData>(sdata) {
                                        Ok(ses_val) => {
                                            // Add session value to sessions list
                                            sessions.insert(sid.clone(), ses_val);

                                            // Send response
                                            ResponseTypes::Success(true).handle_response(Some(CommandTypes::Register), Some(&stream), Some(&mut *sessions), Some(sid.clone()), None)
                                        },
                                        _ => ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason).handle_response( Some(CommandTypes::Register), Some(&stream), None, None, None)
                                    };

                                    if check_sid_u_enc(&sc) {
                                        // When encrypted session was established
                                        if authenticate_user(login, password) {
                                            // update session data
                                            let sd = sc.get(pr.0.as_ref().unwrap()).unwrap();
                                            let mut sd = serde_json::from_str::<SessionData>(sd).unwrap();
                                            sd.connected_to_database = connected_to_db;
                                            update_session_and_res(&pr.0.unwrap(), &sd)
                                        }
                                        else {
                                            ResponseTypes::Error(ErrorResponseKinds::IncorrectLogin).handle_response(Some(CommandTypes::Register), Some(&stream), None, None, None)
                                        }
                                    }
                                    else {
                                        // When encrypted session wasn't established
                                        if authenticate_user(login, password) {
                                            let sid = uuid::Uuid::new_v4().to_string();
                                            let session_data = SessionData {
                                                timestamp: get_timestamp(),
                                                connected_to_database: connected_to_db,
                                                encryption: None
                                            };
                                            update_session_and_res(&sid, &session_data)
                                        }
                                        else {
                                            ResponseTypes::Error(ErrorResponseKinds::IncorrectLogin).handle_response(Some(CommandTypes::Register), Some(&stream), None, None, None)
                                        }
                                    }
                                },
                                CommandTypes::KeepAliveRes(ses_id, timestamp) => { // command to extend session life (heartbeat system -> so keep-alive)                                
                                    let ses_id = {
                                        if let Some(ses_id) = ses_id {
                                            // For not encrypted connection
                                            ses_id
                                        }
                                        else if let Some(ses_id) = pr.0 {
                                            // For encrypted connection
                                            ses_id
                                        }
                                        else {
                                            String::new()
                                        }
                                    };

                                    if ses_id.len() > 0 {
                                        // Session id len can't be empty
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

                                        println!("Session live time has been updated");
                                            // Send response
                                        ResponseTypes::Success(false).handle_response(Some(CommandTypes::KeepAlive), Some(&stream), Some(&mut *sessions), Some(ses_id), None)
                                    }
                                    else {
                                        ResponseTypes::Error(ErrorResponseKinds::IncorrectRequest).handle_response(Some(CommandTypes::KeepAlive), Some(&stream), Some(&mut sessions), Some(ses_id), None)
                                    }
                                    
                                },
                                CommandTypes::CommandRes(query) => {
                                    // Furthermore process query by database
                                    println!("Query: {}", query);

                                    // Send success response to client
                                    ResponseTypes::Success(false).handle_response(Some(CommandTypes::Command), Some(&stream), Some(&mut *sessions), None, Some(query))
                                },
                                CommandTypes::ShowRes(result) => {
                                    println!("Show command Result: {}", result);

                                    ResponseTypes::Success(false).handle_response(Some(CommandTypes::Show), Some(&stream), Some(&mut *sessions), None, Some(result))
                                },
                                CommandTypes::DatabaseConnectRes(database_name, ses_id) => {
                                    let ps = format!("../source/dbs/{}", database_name);
                                    let path_database = Path::new(&ps);
                                    let ses_id = {
                                        if let Some(ses_id) = ses_id {
                                            // For not encrypted connection
                                            ses_id
                                        }
                                        else if let Some(ses_id) = pr.0 {
                                            // For encrypted connection
                                            ses_id
                                        }
                                        else {
                                            String::new()
                                        }
                                    };

                                    if ses_id.len() > 0 {
                                        if path_database.exists() {
                                            let mut sess_datas = serde_json::from_str::<SessionData>(&sessions.get(&ses_id).unwrap()).unwrap();
                                            
                                            // Push database name to session
                                            sess_datas.connected_to_database = Some(database_name);
                                            
                                            // Serialize to string updated session data
                                            let new_sess_content = serde_json::to_string(&sess_datas).unwrap();
    
                                            // Update session data
                                            sessions.insert(ses_id.to_owned(), new_sess_content).unwrap();
    
                                            // Send Response
                                            ResponseTypes::Success(false).handle_response(Some(CommandTypes::DatabaseConnect), Some(&stream), Some(&mut *sessions), Some(ses_id), None)
                                        }
                                        else {
                                            ResponseTypes::Error(ErrorResponseKinds::CouldntPerformQuery("Entered database doesn't exists".to_string())).handle_response(Some(CommandTypes::DatabaseConnect), Some(&stream), Some(&mut *sessions), Some(ses_id), None)
                                        }
                                    }
                                    else {
                                        ResponseTypes::Error(ErrorResponseKinds::IncorrectRequest).handle_response(Some(CommandTypes::DatabaseConnect), Some(&stream), Some(&mut sessions), Some(ses_id), None)
                                    }
                                },
                                _ => () // other types aren't results
                            }
                        },
                        Err(err_kind) => ResponseTypes::Error(err_kind).handle_response(None, Some(&stream), None, None, None)
                    };
                }
                Err(_) => {
                    /* handle probably error */
                    println!("Recived request is incorrect!");
                    if let Err(_) = stream.shutdown(std::net::Shutdown::Both) {
                        println!("Couldn't close TCP connection after handled incorrect response!");
                    }
                }
            }
        }
        else { // while error durning creation of stream handler
            ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason).handle_response(None, None, None, None, None)
        }
    }
}

#[cfg(all(test))]
mod encryption_tests {
    use super::{ CommmunicationEncryption, ConnectionCodec , GenRsaModes };

    #[test]
    fn all_encryptions() {
        // RSA
        let rsa_key_pair = CommmunicationEncryption::gen_rsa_keys(GenRsaModes::Normal).unwrap();
            // Encryption
        let mess_for_rsa = "hello mumy!";
        let mes_enc_rsa_bytes = CommmunicationEncryption::rsa_encrypt_message(mess_for_rsa.to_string()).expect("Couldn't encrypt message using RSA");
        let mes_enc_rsa = ConnectionCodec::code_encrypted_message(mes_enc_rsa_bytes); // after encode to hex string under each character hex code isn't valid utf-8 character!
        println!("RSA encryption: {}", mes_enc_rsa);
            // Decryption
        let dec_rsa_mes = CommmunicationEncryption::rsa_decrypt_message(mes_enc_rsa).expect("Couldn't decrypt message using rsa");
        println!("RSA decryption: {}", dec_rsa_mes);

        print!("\n\n");

        // AES
        let aes_key = CommmunicationEncryption::gen_aes_key();
        let aes_nonce = CommmunicationEncryption::gen_aes_nonce();
        let aes_mess = "Toast smell like a toast!";
            // Encryption
        let enc_aes_mes = CommmunicationEncryption::aes_256_gcm_encrypt(&aes_key, aes_nonce.1.clone(), aes_mess.as_bytes());
        let enc_aes_mes_hex = ConnectionCodec::code_encrypted_message(enc_aes_mes);
        println!("Encrypted AES message in hex string: {}", enc_aes_mes_hex);
            // Decryption
        let dec_mes_in_bytes = ConnectionCodec::decode_encrypted_message(enc_aes_mes_hex).expect("Couldn't decode encrypted AES message from hex to bytes");
        let dec_mes = CommmunicationEncryption::aes_256_gcm_decrypt(&aes_key, aes_nonce.1, dec_mes_in_bytes).expect("Couldn't decode AES encrypted message");
        let dec_mes_string = String::from_utf8(dec_mes).expect("Couldn't create UTF-8 String from decrypted message bytes!");
        println!("Decrypted message content: {}", dec_mes_string)
    }
}
