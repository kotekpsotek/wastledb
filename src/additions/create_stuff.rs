/* Create folders required for dbs data and files for e.i: user accounts, permissions */
use std::{ fs, path::Path as P };
use format as f;
use serde_json::to_string as to_json_string;
use super::login_system::FileDatas as LoginsFileJsonSchema;

pub fn create_stuff() -> Result<(), ()> {
    /* Create folders */
        // folder for datbases and additional infos
    let source_fold = f!("../source");
    if !P::new(&source_fold).exists() {
        fs::create_dir("../source").map_err(|_| ())?;
    }

        // folder for databases
    let dbs_fold = f!("{}/dbs", source_fold);
    if P::new(&dbs_fold).exists() {
        fs::create_dir(dbs_fold).map_err(|_| ())?;
    }

    /* Create files */
        // empty file for login datas
    let logins = f!("{}/logins.json", source_fold);
    if !P::new(&logins).exists() {
        let empty_shema = to_json_string(&LoginsFileJsonSchema { users: vec![] }).map_err(|_| ())?;
        fs::write(logins, empty_shema).map_err(|_| ())?
    }

    Ok(())
}
