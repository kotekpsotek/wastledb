use std::io::{Write, Read};
#[path ="../login-system.rs"]
mod login_system;

use {
    std::net::{ TcpStream, TcpListener },
    std::io::{ BufReader, BufRead }
};
use login_system::authenticate_user;

// 
enum ResponseTypes {
    Success,
    Error(ErrorResponseKinds)
}

impl ResponseTypes {
    fn handle_response(&self, stream: Option<TcpStream>) {
        // Give appropriate action to determined response status
        let mut result_message: String = "NOT".to_string();
            // When below code not handle response type in that case "NOT" response is returned to client
        if matches!(self, ResponseTypes::Error(_)) { // handle not-sucesfull reasons
            if matches!(self, ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason)) || matches!(self, ResponseTypes::Error(ErrorResponseKinds::IncorrectRequest)) { // Handle all for message "Err" response
                let message_type = "Err;";
                if matches!(self, ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason)) {
                    result_message = format!("{}{}", message_type, "UnexpectedReason");
                }
                else { // for all different
                    result_message = format!("{}{}", message_type, "IncorrectRequest");
                }
            }
            else if matches!(self, ResponseTypes::Error(ErrorResponseKinds::IncorrectLogin)) {
                result_message = String::from("IncLogin;Null")
            }
        }
        else if matches!(self, ResponseTypes::Success) {
            result_message = "OK".to_string();
        }

        // Send response
            // or inform that response can't be send
        let couldnt_send_response = || {
            println!("Couldn't send response to client. Error durning try to create handler for \"TCP stream\"")
        };
        match stream {
            Some(mut stri) => {
                let resp = stri.write_all(result_message.as_bytes());
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
    IncorrectLogin // when user add incorrect login data or incorrect login data format (this difference is important)
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
    RegisterRes(LoginCommandData) // Result of parsing "Register" command recognizer prior as "Register" child
}

struct CommandTypeKeyDiff<'s> { 
    name: &'s str,
    value: &'s str
}

impl CommandTypes {
    // Parse datas from recived request command
    // Return command type and its data such as login data
    fn parse_cmd(&self, msg_body: &str) -> Result<CommandTypes, ErrorResponseKinds> {
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
                    Err(ErrorResponseKinds::IncorrectLogin)
                }
            }
            else {
                Err(ErrorResponseKinds::IncorrectLogin)
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

// Convert request to vector
// "Call as 1"
fn convert_request(mut stream: &TcpStream) -> Option<Vec<String>> {
    let buf_reader = BufReader::new(&mut stream);
    let tcp_request: Vec<String> = buf_reader
        .lines()
        .map(|result| {
            match result {
                Ok(data) => data,
                Err(_) => String::new()
            }
        })
        .take_while(|line| !line.is_empty())
        .collect();
    // return data forward
    if tcp_request.len() == 0 {
        return None
    }

    Some(tcp_request)
}

// "Call as 2"
// Recoginize commands, parse it and return Ok() when both steps was berformed correctly or return Err() when these both steps couldn't be performed. Error is returned as ErrorResponseKinds enum which can be handled directly by put it into enum "ResponseTypes" and call to method ".handle_response(tcp_stream)"
fn parse_request(converted_request: Vec<String>) -> Result<CommandTypes, ErrorResponseKinds> {
    let message = &converted_request[0]; // message can't be empty here
    let message_semi_spli = message.split(";").collect::<Vec<&str>>(); // split message using semicolon
    if message_semi_spli.len() > 1 { // must be at least 2 pieces: "Message Type" and second in LTF order "Message Body"
            // mes type
        let message_type = message_semi_spli[0].to_lowercase();
        let message_type = message_type.as_str();
            // mes body
        let message_body = message_semi_spli[1];
       
        // Handle command
        /* if message_type == "command" { // Execute command into db here
            Ok(CommandTypes)
        }
        else */ if message_type == "register" { // login user into database and save his session
            match CommandTypes::Register.parse_cmd(message_body) {
                Ok(reg_cmd) => {
                    Ok(reg_cmd) // Return: CommandTypes::RegisterRes(LoginCommandData { login: String::new("login datas"), password: String::new("password datas") })
                },
                Err(err) => {
                    Err(err)
                }
            }
        }
        /* else if message_type == "keep-alive" { // keep user session saved when 

        } */
        else { // when unsuported message was sended
            Err(ErrorResponseKinds::IncorrectRequest)
        }
    }
    else { // when after separation message by its parts exists less or equal to 1 part in Vector
        Err(ErrorResponseKinds::IncorrectRequest)
    }
}

// "Call from outside to connect all chunks together"
pub fn handle_tcp(port: &str) {
    let listener = TcpListener::bind("192.168.0.25:20050").expect("Couldn't spawn TCP Server on selected port!");

    for request in listener.incoming() {
        if let Ok(stream) = request {
            let converted_req = convert_request(&stream);

            if converted_req.is_some() {
                let parse_request = parse_request(converted_req.unwrap());
                match parse_request {
                    Ok(command) => { // All supported and correct parsed commands returns Ok()
                        match command {
                            CommandTypes::RegisterRes(login_data) => { // Recived when user call to "register" command
                                if authenticate_user(login_data.login, login_data.password) {
                                    ResponseTypes::Success.handle_response(Some(stream))
                                }
                                else {
                                    ResponseTypes::Error(ErrorResponseKinds::IncorrectLogin).handle_response(Some(stream))
                                }
                            },
                            _ => () // unsuported commands doesn't handled here, here are handled only correctly recognized and parsed commnands. All incorrect commands are parsed by below "Err() "arm 
                        }
                    }
                    Err(err_kind) => { // All incorrect  parsed messages are handled using this arm. Benath "handle_response(Some(stream))" method is systemt to handle all responses!!!
                        ResponseTypes::Error(err_kind).handle_response(Some(stream))
                    }
                }
            }
            else { // while request packet is empty
                ResponseTypes::Error(ErrorResponseKinds::IncorrectRequest).handle_response(Some(stream))
            }
        }
        else { // while error durning creation of stream handler
            ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason).handle_response(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn res_types_check_methods() {
        ResponseTypes::Error(ErrorResponseKinds::UnexpectedReason).handle_response(None)
    }
}
