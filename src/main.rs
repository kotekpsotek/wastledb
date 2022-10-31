mod connection {
    pub mod tcp;
}
mod tui;
#[path ="./login-system.rs"]
mod login_system;
mod inter;

use clap::{ Command, Arg, ArgAction };

fn main() {
    // tui::tui_create();
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
        connection::tcp::handle_tcp();
    }
    else {
        connection::tcp::handle_tcp();
    }
}

// WARNING: To start robust testing you must turn on tcp server first
#[cfg(test)]
mod tests {
    #[path = "../login-system.rs"]
    mod login_module;

    use std::{ net::TcpStream, io::{Write, Read, BufReader, BufRead} };
    use login_module::authenticate_user;
    use std::str;
    use format as f;
    use super::inter::MAXIMUM_RESPONSE_SIZE_BYTES;

    fn register_user_by_tcp() -> String {
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");
        
        // Request
        connection.write("Register;login|x=x|tester 1-1 password|x=x|123456789 1-1 connect_auto|x=x|dogo".as_bytes()).unwrap();

        // Response
        let mut buf = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf).expect("Couldn't read server response");

        let resp_str = String::from_utf8(buf.to_vec()).unwrap();
        
        // Return response
        resp_str
    }

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
        let sess_id = registered_response.split(";").collect::<Vec<&str>>()[1].replace("\0", "");
        
        // Second call (source)
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");

            //... Request
        connection.write(f!("Keep-Alive;{}", sess_id).as_bytes()).unwrap();

            //... Response
        let mut buf2 = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf2).expect("Couldn't read server response");
        let resp2_str = String::from_utf8(buf2.to_vec()).unwrap();
        println!("{}", resp2_str)
    }

    #[test]
    fn tcp_command_cmd() {
        // First call = Register user
        let registered_response = register_user_by_tcp();
            //... session id in form without \0 (empty) characters
        let sess_id = registered_response.split(";").collect::<Vec<&str>>()[1].replace("\0", "");

        // Second call (source)
        let mut connection = TcpStream::connect("127.0.0.1:20050").expect("Couldn't connect with server");

            //... Request 
            // Remained options (not used in sended query) (with separators): 1-1 connect_auto|x=x|true
        connection.write(f!("Command;session_id|x=x|{} 1-1 sql_query|x=x|CREATE TABLE dogo123", sess_id).as_bytes()).unwrap();

            //... Response
        let mut buf2 = [0; MAXIMUM_RESPONSE_SIZE_BYTES];
        connection.read(&mut buf2).expect("Couldn't read server response");
        
                // ... parse response buf to String
        let resp2_str = String::from_utf8(buf2.to_vec()).unwrap();
        println!("{}", resp2_str)
    }

    #[test]
    fn test_authenticate_user() {
        let test_login = "tester".to_string();
        let test_password = "1234567890".to_string();
        println!("Authentication result: {}", authenticate_user(test_login, test_password));
    }
}
