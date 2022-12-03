use std::io::{BufRead, BufReader};
use std::vec;
use std::{ self, fs, io::Read };
use serde::{Serialize, Deserialize, self};
use serde_json;
use sha3::{ self, Digest };
use std::format as f;
use std::str;
use std::fmt::Write;
use encoding_rs::*;
const FILE_WITH_LOGIN_DATAS: &str = "../logins.json";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OneUser {
    login: String,
    password: String,
    permission_group: String
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileDatas {
    pub users: Vec<OneUser>
}

struct LoginSecurity;
impl LoginSecurity {
    fn hash(data: OneUser) -> (Vec<u8>, Vec<u8>) { // Returned: 1. hashed login, 2. hashed password 
        let mut sha3_hash = sha3::Sha3_512::default();
        let mut en_pass: bool = false;
        let mut en_log: bool = false;

        // Hash password
        sha3_hash.update(data.password.as_bytes());
        let password_raw = &sha3_hash.clone().finalize()[..];

        // Hash login
        sha3_hash.update(data.login.as_bytes());
        let login_raw = &sha3_hash.finalize()[..];
        
        (login_raw.to_vec(), password_raw.to_vec())
    }
}

// pub fn create_user(login: String, password: String) -> bool {

// }

pub fn authenticate_user(login: String, password: String) -> bool {
    if !self::check_user_data_correctenss(login, password) {
        return false;
    };

    return true;
}

fn check_user_data_correctenss(login: String, password: String,) -> bool {
    let serialized_userfile_data = self::read_users_file();
    for user in serialized_userfile_data.users {
        let checked_data_encr = LoginSecurity::hash(OneUser { login: login.clone(), password: password.clone(), permission_group: f!("") });
        let checked_data_encr_login = convert_bytes_to_hex_string(&checked_data_encr.0[..]);
        let checked_data_encr_pass = convert_bytes_to_hex_string(&checked_data_encr.1[..]);
        
        if user.login == checked_data_encr_login {
            return user.password == checked_data_encr_pass; // Return value from "for" loop yoooooo....
        }
    };

    false
}

fn read_users_file() -> FileDatas {
    let file = fs::read_to_string(FILE_WITH_LOGIN_DATAS).unwrap();
    serde_json::from_str::<FileDatas>(file.as_str()).unwrap()
}

fn convert_bytes_to_hex_string(bytes: &[u8]) -> String { // convert utf-8 bytes to hexdecimal bytes and put them into string. Function is used in order to prevent from "incorrect utf-8 character error" (invoked after hash data)
    let mut hex_login = String::new();
    for byte in bytes {
        write!(hex_login, "{:#04X?} ", byte).expect("Unable to write");
    };
    hex_login
}

// Create user and return opration result in boolean form
pub fn create_user(login: String, password: String, permission_group: Option<&String>) -> bool { // Return operation result in boolean form
    // User Datas from file
    let mut already_saved_in = self::read_users_file();

    // Pre-Prepare new user datas
    let encr_user_confidential_data = LoginSecurity::hash(OneUser { login, password, permission_group: f!("") });
    let hex_login = convert_bytes_to_hex_string(&encr_user_confidential_data.0[..]);
    let hex_password = convert_bytes_to_hex_string(&encr_user_confidential_data.1[..]);
    let mut same_user_exists = false;

    // Prevent from from create user with same login
    for user in &already_saved_in.users {
        if user.login == hex_login {
            same_user_exists = true;
        };
    };
    
    if same_user_exists {
        return false;
    }

    // Prepare new user datas - ready to save
    let user_struct = OneUser {
        login: hex_login,
        password: hex_password,
        permission_group: match permission_group {
            Some(pgroup) => pgroup.clone(),
            None => f!("")
        }
    };

    // Save prepared datas
    let with_new_user = vec![already_saved_in.users, Vec::from([user_struct])].concat();
    already_saved_in.users = with_new_user;
    
        // ...to json format
    let ready_json = serde_json::to_string_pretty(&already_saved_in);
    if let Err(_) = ready_json {
        return false
    };

        // ...save result to file and return save operation result (as boolean) from function
    fs::write(FILE_WITH_LOGIN_DATAS, ready_json.unwrap())
        .map_or_else(|_e| false, |_s| true) // remap Result<> to boolean values on each case
}
