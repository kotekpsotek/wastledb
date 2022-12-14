mod connection {
    pub mod tcp;
}
#[path ="./login-system.rs"]
mod login_system;
#[path ="./additions/create_stuff.rs"]
mod create_stuff;
mod inter;
use tokio;

use clap::{ Command, Arg, ArgAction };

mod management {
    pub mod sql_json;
}

#[tokio::main]
async fn main() {
    // Create required folders and files when don't exists
    create_stuff::create_stuff().expect("Couldn't create files and directories required to duly Database working!");

    // CLI
    let add_user = Command::new("database TUI interface")
        .about("Create new database user")
        .subcommand(
            Command::new("adu")
                    .about("Add new user to databse users list")
                    .args([
                        Arg::new("login")
                            .long("login")
                            .short('l')
                            .action(ArgAction::Set)
                            .long_help("login for new user. This value will be using as user \"login\"")
                            .required(true),
                        Arg::new("password")
                            .long("password")
                            .short('p')
                            .action(ArgAction::Set)
                            .long_help("password for new user. This value will be using as user \"password\"")
                            .required(true),
                        Arg::new("permission")
                            .short('a')
                            .action(ArgAction::Set)
                            .long_help("Setup specific permision group for user")
                            .required(false)
                    ])
        )
        .subcommand(
        Command::new("run")
                    .about("Run database")
                    .version("1.0")
        )
        .get_matches();

    if let Some(cmd) = add_user.subcommand_matches("adu") {
        if let Some(login) = cmd.get_one::<String>("login") {
            if let Some(password) = cmd.get_one::<String>("password") {
                if login.len() > 0 && password.len() >= 8 {
                    let permision_grade: Option<&String> = cmd.get_one::<String>("permission");
                    // println!("Correct user data added!")
                    // Encrypt user data and save it into file with users datas
                    if !login_system::create_user(login.clone(), password.clone(), permision_grade) {
                        println!("From some reason couldn't create new user! Try again!");
                    }
                    else {
                        println!("Created new user!");
                    };
                } 
                else if password.len() < 8 {
                    println!("In order to give appropriate security pitch you should enter password contained from more then 8 characters or from only 8 characters when you sure with less safeguards")
                }
                else {
                    println!("Login must be created from at least one UTF-8 character");
                }
            };
        }
    }
    else if let Some(_) = add_user.subcommand_matches("run") {
        connection::tcp::handle_tcp().await;
    }
    else {
        connection::tcp::handle_tcp().await;
    }
}

// WARNING: To start robust testing you must turn on tcp server first
#[cfg(test)]
pub mod tests {
    #[path = "../login-system.rs"]
    mod login_module;

    use std::{ net::TcpStream, io::{Write, Read, BufReader, BufRead}, borrow::Borrow, fmt::format };
    use login_module::authenticate_user;
    use std::str;
    use format as f;
    use crate::connection::tcp::CommmunicationEncryption;

    use super::inter::MAXIMUM_RESPONSE_SIZE_BYTES;

    use rsa::{self,  pkcs1::{self, DecodeRsaPublicKey, DecodeRsaPrivateKey} };
    use datafusion::prelude::*;
    use tokio;
    use super::connection::tcp::ConnectionCodec;

    pub fn register_user_by_tcp() -> String {
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");
        
        // Request
            // When rsa option is picked (rsa|x=x|true ["|x=x|" is key->value separator]) then publick key wil be recived in response as last option
        let as_hex = ConnectionCodec::code_hex("Register;login|x=x|tester 1-1 password|x=x|123456789 1-1 connect_auto|x=x|dogo".to_string());
        connection.write(as_hex.as_bytes()).unwrap();

        // Response
        let mut buf = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf).expect("Couldn't read server response");

        let resp_str = String::from_utf8(buf.to_vec()).expect("Coulnd't create utf-8 string with HEX codes string").replace("\0", ""); // + replace null character for elminate error durning decoding code to utf-8 cuz: in HEX letters range (0-F) control character \0 is absent
        let from_hex = ConnectionCodec::decode_hex(resp_str).expect("Couldn't decode response from HEX");
        
        // // Return response
        from_hex
    }

    /// Convert message body to ready to use passages
    /// fn(register_message_body) -> (session_id, rsa_public_key (or None)) 
    fn parse_register_response_body(body: String) -> (String, Option<rsa::RsaPublicKey>) {
        let res_body = body.split(";").collect::<Vec<&str>>()[1].replace("\0", "");
        let keys_vals_separation = res_body.split("|x=x|").collect::<Vec<&str>>();

        let sess_id = String::from(keys_vals_separation[0]); // sess id always first
        let public_key = { // public key should be 2 in callback body and it must be correct public key to be returned
            if keys_vals_separation.len() > 1 {
                if let Ok(key) = rsa::RsaPublicKey::from_pkcs1_pem(keys_vals_separation[1]) {
                    Some(key)
                }
                else {
                    None
                }
            }
            else {
                None
            }
        };

        (sess_id, public_key)
    }

    // TODO: Change TCP command: It require encode sending payload to valid utf-8 (not-encrypted so use for it function: ConnectionCodec::code_hex()) hex before send it to DBS
    #[test]
    fn tcp_tester() {
        let mut stream = TcpStream::connect("0.0.0.0:20050").expect("Couldn't connect with server");
        let _on1 = stream.write("Siemanko".as_bytes()).unwrap();
        // stream.shutdown(std::net::Shutdown::Both).unwrap();
        let _on2 = stream.write("Siemanko 2".as_bytes()).expect("Caused on 2 try");
        let rcnt: &mut [u8] = &mut [1];

        std::thread::sleep(std::time::Duration::from_millis(100));
        stream.read(rcnt).unwrap();
    }

    #[test]
    fn tcp_register_cmd() {
        let response = register_user_by_tcp();
        println!("Response is: {:?}", response); // Same 0's = no response from server
    }

    #[test]
    fn tcp_keepalive_cmd() {
        // First call = Register user
        let registered_response = register_user_by_tcp();
            //... session id in form without \0 (empty) characters
        let sess_id = parse_register_response_body(registered_response).0;
        println!("{}", sess_id);
        
        // Second call (source)
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");

            //... Request
        let command = f!("Keep-Alive;{}", sess_id);
        let command = ConnectionCodec::code_hex(command);
        connection.write(command.as_bytes()).unwrap();

            //... Response
        let mut buf2 = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf2).expect("Couldn't read server response");
        let resp2_str = String::from_utf8(buf2.to_vec()).unwrap(); // Session response is encoded into hex
        let resp2_str = ConnectionCodec::decode_hex(resp2_str).expect("couldn't decode message");
        println!("{}", resp2_str)
    }

    #[test]
    fn tcp_command_cmd() {
        // First call = Register user
        let registered_response = register_user_by_tcp();
            //... session id in form without \0 (empty) characters
        let sess_id = parse_register_response_body(registered_response).0;

        // Second call (source)
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");

            //... Request 
        // Remained options (not used in sended query) (with separators): 1-1 connect_auto|x=x|true
                // ... Operation: INSERT INTO    
        // connection.write(f!(r#"Command;sql_query|x=x|INSERT INTO "mycat2" VALUES ('kika', 'female', 5) 1-1 session_id|x=x|{}"#, sess_id).as_bytes()).unwrap();
                // ... Opeartion: INSERT OVERWRITE TABLE
        // connection.write(f!(r#"Command;sql_query|x=x|INSERT OVERWRITE TABLE "mycat" VALUES ('cat', 'xx', '1') 1-1 session_id|x=x|{}"#, sess_id).as_bytes()).unwrap();
                // ... Operation: CREATE TABLE
        // connection.write(f!(r#"Command;sql_query|x=x|CREATE TABLE mycat2 (name varchar(255) NOT NULL, gender varchar(255) NOT NULL, age int) 1-1 session_id|x=x|{}"#, sess_id).as_bytes()).unwrap();
        // ... Operation: TRUNCATE
        // connection.write(f!(r#"Command;sql_query|x=x|TRUNCATE TABLE mycat 1-1 session_id|x=x|{}"#, sess_id).as_bytes()).unwrap();
                // ... Operation: Drop
        // connection.write(f!(r#"Command;sql_query|x=x|DROP TABLE mycat 1-1 session_id|x=x|{}"#, sess_id).as_bytes()).unwrap();
                // ... Operation: SELECT
        // connection.write(f!(r#"Command;sql_query|x=x|SELECT * FROM mycat2 WHERE age = 5 AND name = kika OR age = 2 1-1 session_id|x=x|{}"#, sess_id).as_bytes()).unwrap();
                // ... Operation: DELETE
        // connection.write(f!(r#"Command;sql_query|x=x|DELETE FROM mycat2 WHERE age >= 2 1-1 session_id|x=x|{}"#, sess_id).as_bytes()).unwrap();
                // ... Operation: UPDATE
        // connection.write(f!(r#"Command;sql_query|x=x|UPDATE mycat2 SET name = 'hex', age = 255 1-1 session_id|x=x|{}"#, sess_id).as_bytes()).unwrap();
        // ... Operation: ALTER TABLE // TODO: More spohisticated test for query 'ALTER TABLE .. CHANGE COLUMN ..'
        // connection.write(f!(r#"Command;sql_query|x=x|ALTER TABLE mycat2 CHANGE COLUMN name_test name varchar(2555) 1-1 session_id|x=x|{}"#, sess_id).as_bytes()).unwrap();
            // Command ALTER TABLE couldn't be parsed by sqlparser (always SQL Syntax Error)
        let command = f!("Command;sql_query|x=x|ALTER TABLE t2 ALTER COLUMN c varchar(355) 1-1 session_id|x=x|{}", sess_id);
        let command = ConnectionCodec::code_hex(command); // data must be in hex format
        connection.write(command.as_bytes()).unwrap();
            //... Response
        let mut buf2 = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf2).expect("Couldn't read server response");
        
                // ... parse response buf to String
        let resp2_str = String::from_utf8(buf2.to_vec()).unwrap();
        let resp2_str = ConnectionCodec::decode_hex(resp2_str).expect("couldn't decode message");
        println!("{}", resp2_str)
    }

    #[test]
    fn tcp_show_cmd() {
        // First call = Register user
        let registered_response = register_user_by_tcp();
            //... session id in form without \0 (empty) characters
        let sess_id = parse_register_response_body(registered_response).0;

        // Second call (source)
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");

        // Request
            // Show dbs databases
        let command = f!(r#"Show;what|x=x|databases 1-1 unit_name|x=x|none 1-1 session_id|x=x|{}"#, sess_id);
            // Show database tables
        // let command = f!(r#"Show;what|x=x|database_tables 1-1 unit_name|x=x|none 1-1 session_id|x=x|{}"#, sess_id);
            // Show specific table data
        // let command = f!(r#"Show;what|x=x|table_records 1-1 unit_name|x=x|mycat2 1-1 session_id|x=x|{}"#, sess_id);
        let command = ConnectionCodec::code_hex(command);
        connection.write(command.as_bytes()).unwrap();

        // Response
        let mut buf2 = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf2).expect("Couldn't read server response");
            // Parse response buf to String
        let resp2_str = String::from_utf8(buf2.to_vec()).unwrap();
        let resp2_str = ConnectionCodec::decode_hex(resp2_str).expect("couldn't decode message");
        
        println!("{}", resp2_str)
    }

    #[test]
    fn tcp_databaseconnect_cmd() {
        // First call = Register user
        let registered_response = register_user_by_tcp();
            //... session id in form without \0 (empty) characters
        let sess_id = parse_register_response_body(registered_response).0;

        // Second call (source)
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");

        // Request
        let command = f!(r#"DatabaseConnect;database_name|x=x|test 1-1 session_id|x=x|{}"#, sess_id);
        let command = ConnectionCodec::code_hex(command);
        connection.write(command.as_bytes()).unwrap();

        // Response
        let mut buf2 = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf2).expect("Couldn't read server response");
            // Parse response buf to String
        let resp2_str = String::from_utf8(buf2.to_vec()).unwrap();
        let resp2_str = ConnectionCodec::decode_hex(resp2_str).expect("couldn't decode message");
        println!("{}", resp2_str)
    }

    #[test]
    fn test_authenticate_user() {
        let test_login = "tester".to_string();
        let test_password = "1234567890".to_string();
        println!("Authentication result: {}", authenticate_user(test_login, test_password));
    }

    // Tests on encrypted connection

    /// Parsed key from encrypted connection
    type Key = (String, String);
    /// Encrypt user connection
    fn encrypted_connection() -> (Key, Key, Key) { // -> AesKey, Nonce, Session ID
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");
        
        // Request
            // When rsa option is picked (rsa|x=x|true ["|x=x|" is key->value separator]) then publick key wil be recived in response as last option
        let as_hex = ConnectionCodec::code_hex("InitializeEncryption;".to_string());
        connection.write(as_hex.as_bytes()).unwrap();
    
        // Response
        let mut buf = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf).expect("Couldn't read server response");
    
        let resp_str = String::from_utf8(buf.to_vec()).expect("Coulnd't create utf-8 string with HEX codes string").replace("\0", ""); // + replace null character for elminate error durning decoding code to utf-8 cuz: in HEX letters range (0-F) control character \0 is absent
        let dec_stri = CommmunicationEncryption::rsa_decrypt_message(resp_str).expect("Couldn't decrypt message from rsa!");
    
        // Parse decrypted stri
        let sli = dec_stri.split(";").collect::<Vec<_>>()[1].split(" 1-1 ").collect::<Vec<_>>();
        let parse_key_value = |key: usize| {
            let choosen = sli[key].split("|x=x|").collect::<Vec<_>>();
        
            // key name, key value
            (choosen[0].to_string(), choosen[1].to_string())
        };
        let parsed = (parse_key_value(0), parse_key_value(1), parse_key_value(2));
        parsed
    }
    
    fn register_secure((aes, nonce, session_id): (Key, Key, Key)) {
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");
        
        // Request
            // Prepare message to send
        let mes_cont = "login|x=x|tester 1-1 password|x=x|123456789 1-1 connect_auto|x=x|dogo".to_string();
        let mes_enc = CommmunicationEncryption::aes_256_gcm_encrypt(&aes.1, ConnectionCodec::decode_encrypted_message(nonce.1.clone()).expect("couldn't decode nonce to vector"), mes_cont.as_bytes());
        let mes_encoded = ConnectionCodec::code_encrypted_message(mes_enc);
        let mess_form = format!("{com};{ms};{session_id}", com = "Register", ms = mes_encoded, session_id = session_id.1);
        let mes_ready = ConnectionCodec::code_hex(mess_form);
        
            // Send request to dbs
        connection.write(mes_ready.as_bytes()).unwrap();

        // Response
        let mut buf = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf).expect("Couldn't read server response");

        let resp_str = String::from_utf8(buf.to_vec()).expect("Coulnd't create utf-8 string with HEX codes string").replace("\0", ""); // + replace null character for elminate error durning decoding code to utf-8 cuz: in HEX letters range (0-F) control character \0 is absent
        let to_not_validutf8_hex = ConnectionCodec::decode_encrypted_message(resp_str).expect("Couldn't decode response from HEX"); // Decode hex to ciphertext (not valid utf-8)
        
        // Decode response
        let dec_res = CommmunicationEncryption::aes_256_gcm_decrypt(&aes.1,  ConnectionCodec::decode_encrypted_message(nonce.1).expect("Couldn't decode nonce"), to_not_validutf8_hex).expect("Couldn't decode response");
        let dec_res_stri = String::from_utf8(dec_res).expect("Couldn't create UTF-8 string from decoded message");

        // // Return response
        println!("{}", dec_res_stri)
    }

    #[test]
    fn tcp_initialize_encryption() { // test command which is for intiialize encryption between 2 communication sites
        let _ = self::encrypted_connection(); // AesKey, Nonce, Session ID
    }

    #[test]
    fn tcp_register_secure() {
        let data_to_ecn_connection = self::encrypted_connection(); 
        self::register_secure(data_to_ecn_connection)
    }
}
