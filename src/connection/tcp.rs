use std::{io::{Write, Read}, fmt::format};
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
    std::time::SystemTime
};
use login_system::authenticate_user;
use uuid::{Uuid, timestamp};
use crate::inter;
use management::main::Outcomes::*;

// 
enum ResponseTypes {
    Success(bool), // 1. whether attach to success response message user sess id
    Error(ErrorResponseKinds)
}

impl ResponseTypes {
    fn handle_response(&self, stream: Option<TcpStream>, session_id: Option<String>) {
        // Give appropriate action to determined response status
        let mut result_message: String = "NOT".to_string();
            // When below code not handle response type in that case "NOT" response is returned to client
        if matches!(self, ResponseTypes::Error(_)) { 
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
        else if matches!(self, ResponseTypes::Success(_)) {
            result_message = if matches!(self, ResponseTypes::Success(true)) {
                    //...here session id must be attached to method call
                let session_id = session_id.expect(&format!("You must attach session id to \"{}\" method in order to handle correct results when {}", stringify!(self.handle_response), stringify!(Self::Success(true))));
                format!("OK;{}", session_id).to_string()
            }
            else {
                "OK".to_string()
            };
        }

        // Send response
            // or inform that response can't be send
        let couldnt_send_response = || {
            println!("Couldn't send response to client. Error durning try to create handler for \"TCP stream\"")
        };
        match stream {
            Some(mut stri) => {
                let resp = stri.write(result_message.as_bytes());
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
struct LoginCommandData { // Login user data
    login: String,
    password: String
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

struct CommandTypeKeyDiff<'s> { 
    name: &'s str,
    value: &'s str
}

impl CommandTypes {
    // Parse datas from recived request command
    // Return command type and its data such as login data
    // SQL query are processed inside
    #[allow(unused_must_use)] // Err should be ignored only for inside call where are confidence of correcteness
    fn parse_cmd(&self, msg_body: &str, sessions: Option<&mut HashMap<String, String>>) -> Result<CommandTypes, ErrorResponseKinds> {
        if matches!(self, Self::Register) { // command to login user
            if msg_body.len() > 0 {
                let msg_body_sep = msg_body.split(" 1-1 ").collect::<Vec<&str>>();
                if msg_body_sep.len() == 2 { // isnide login section must be 2 pieces: 1 - login|x=x|logindata 2 - password|x=x|passworddata
                    let mut keys_required_list = LoginCommandData { login: String::new(), password: String::new() };

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
            if msg_body_sep.len() == 2 { // isnide login section must be 2 pieces: 1 - session_id|x=x|session_id 2 - sql_query|x=x|sql query which will be executed on db
                //... session
                let request_session = self
                    .clone()
                    .parse_key_value(msg_body_sep[0]);

                if let Some(CommandTypeKeyDiff { name, value }) = request_session {
                    let sessions = sessions.unwrap(); 
                    if name == "session_id" && sessions.contains_key(value) { // key "session_id" must always be first
                        //... query
                        let sql_query_key = self
                            .clone()
                            .parse_key_value(msg_body_sep[1]);
                        
                        if let Some(CommandTypeKeyDiff { name, value }) = sql_query_key {
                            if name == "sql_query" && value.len() > 0 {
                                // Extend session live time after call this command
                                CommandTypes::KeepAlive.parse_cmd(msg_body, Some(sessions));

                                // Process query
                                let q_processed_r = self::management::main::process_query(value);
                                match q_processed_r {
                                    Success(desc_opt) => {
                                        if let Some(desc) = desc_opt {
                                            return Ok(CommandTypes::CommandRes(desc))
                                        };

                                        Ok(CommandTypes::CommandRes(format!("Query has been performed")))
                                    },
                                    Error(reason) => Err(ErrorResponseKinds::CouldntPerformQuery(reason))
                                }

                                // Correct query response
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
struct SessionData {
    timestamp: u128
}

/* struct SessionHandler;
impl SessionHandler {
    
} */

// Callculate timestamp (how much milliseconds flow from 1 January 1970)
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
pub fn handle_tcp() {
    let tcp_server_adress = format!("0.0.0.0:{port}", port = inter::TCP_PORT);
    let listener = TcpListener::bind(tcp_server_adress).expect("Couldn't spawn TCP Server on selected port!");
    let mut sessions = HashMap::<String, String>::new(); // key - session id, data - session data in json format

    for request in listener.incoming() {
        if let Ok(mut stream) = request {
            match handle_request(&mut stream) {
                Ok(c_req) => {
                    /* Do more... */
                    match process_request(c_req.clone(), Some(&mut sessions)) {
                        Ok(command_type) => {
                            match command_type {
                                // Save user session
                                CommandTypes::RegisterRes(LoginCommandData { login, password }) => {
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
                                            timestamp: get_timestamp()
                                        };
                                            //... Serialize session data into JSON, handle result and send appropriate response to what is Result<..> outcome
                                        match serde_json::to_string(&session_data) {
                                            Ok(ses_val) => {
                                                // Add session value to sessions list
                                                sessions.insert(uuid_gen.clone(), ses_val);
                                                // println!("{:?}", sessions); // Test: print all sessions in list after add new session

                                                // Send response
                                                ResponseTypes::Success(true).handle_response(s, Some(uuid_gen))
                                            },
                                            _ => ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason).handle_response(s, None)
                                        };
                                    }
                                    else {
                                        ResponseTypes::Error(ErrorResponseKinds::IncorrectLogin).handle_response(s, None)
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
                                    ResponseTypes::Success(false).handle_response(Some(stream), Some(ses_id))
                                },
                                CommandTypes::CommandRes(query) => {
                                    // Furthermore process query by database
                                    println!("Query: {}", query);

                                    // Send success response to client
                                    ResponseTypes::Success(false).handle_response(Some(stream), None)
                                }
                                _ => () // other types aren't results
                            }
                        },
                        Err(err_kind) => ResponseTypes::Error(err_kind).handle_response(Some(stream), None)
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
            ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason).handle_response(None, None)
        }
    }
}
