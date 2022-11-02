#![allow(unused)]
use std::{fs, path::Path};

use serde::{self, Deserialize, Serialize};
use sqlparser::{
    self,
    ast::{ColumnOption, ColumnOptionDef, DataType, Statement},
};
use Statement::*;

/* Create table in json format */
#[derive(Serialize, Deserialize, Debug, Clone)]
/// Represent SQL table created from query in JSON format
pub struct JsonSQLTable {
    /// table name
    pub name: String,
    /// columns schema
    pub columns: Vec<JsonSQLTableColumn>,
    /// rows with columns. This value can be represented by None in moment when: table is now created or it doesn't have got any records inside
    pub rows: Option<Vec<Vec<JsonSQLTableColumnRow>>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
/// Represent all supported SQL collumn types in this database
pub enum SupportedSQLDataTypes {
    INT,
    FLOAT,
    TEXT,
    VARCHAR(Option<u16>), // can store maximum 65_535 bytes
    LONGTEXT,
    DATE,
    DATETIMESTAMP,
    NULL,
    BOOLEAN,
}

/// Trait to easy and seamlessly convertion between SQLParser Types stored benath "DataTypes" enum and supported types by database
pub trait ConvertSQLParserTypesToSupported {
    fn convert(parser_type: &DataType) -> Option<SupportedSQLDataTypes> {
        use DataType::*;
        // TODO: Add support for more types
        match parser_type {
            Varchar(prop) => {
                if let Some(prop) = prop {
                    let len = prop.length;
                    Some(SupportedSQLDataTypes::VARCHAR(Some(len as u16))) // u16 is sufficient becuse varchar can store maximum 65_535 characters
                }
                else {
                    Some(SupportedSQLDataTypes::VARCHAR(None))
                }
            },
            Int(_width) => { // TODO: add support for Int width
                Some(SupportedSQLDataTypes::INT)
            },
            Text => {
                Some(SupportedSQLDataTypes::TEXT)
            },
            _ => None // unsuported
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(non_camel_case_types)]
/// Represent all supported column constraints in this database
pub enum SupportedSQLColumnConstraints {
    PRIMARY_KEY,
    FOREGIN_KEY,
    DEFAULT(String),
    NOT_NULL,
    UNIQUE,
}

/// Trait to easy and seamlessly convertion between SQLParser Options (in sqlparser and is equal concept to Constraint) stored benath "ColumnOption.option" as a "ColumnOption" enum to supported constraints by database
pub trait ConvertSQLParserOptionsToSupportedConstraints {
    fn convert(option: sqlparser::ast::ColumnOptionDef) -> Option<SupportedSQLColumnConstraints> {
        use sqlparser::ast::ColumnOption::*;
        // TODO: Add support for more constraints
        match option.option {
            NotNull => Some(SupportedSQLColumnConstraints::NOT_NULL),
            _ => None // for unsuppored options
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
/// Represent each column placed in JsonSQLTable
pub struct JsonSQLTableColumn {
    /// column name
    pub name: String,
    /// column data type
    pub d_type: SupportedSQLDataTypes,
    /// optional column constraints
    pub constraints: Option<Vec<SupportedSQLColumnConstraints>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
/// Represent each row with data for "JsonSQLTable" struct
pub struct JsonSQLTableColumnRow {
    pub col: String,
    pub value: String,
}

#[derive(Debug, PartialEq)]
pub struct ProcessSQLRowField(pub String, pub SupportedSQLDataTypes); // 1. field value, 2. Field data type (only supported datatypes)

type TableName = String;
type TablePath<'s> = &'s Path;
type ColumnName = String;
type ActionOnlyForTheseColumns = Vec<String>;
type RowsToProcess = Vec<ProcessSQLRowField>;

#[derive(Debug)]
pub enum ProcessSQLSupportedQueries<'x> {
    Insert(
        TablePath<'x>,
        Option<ActionOnlyForTheseColumns>,
        Vec<RowsToProcess>,
    ), // 1. Table name, 2. Optional: Insert only for specified here column names, 3. List with rows values (which will be attached)
    CreateTable(
        TableName,
        Vec<(
            ColumnName,
            SupportedSQLDataTypes,
            Option<Vec<SupportedSQLColumnConstraints>>,
        )>,
    ), // 1. Table name, 2. Vector with table columns and characteristic for each column
}

// TODO: Change description
/// Processing attached SQL query and returns its result as "JsonSQLTable" type ready to serialize to json format thanks to "serde" and "serde_json" crates
/// When something went bad durning analyze or processing sql query then Error without any description is returned
// Note: Polish characters are not supported by sqlparser, so not use them into queries
#[must_use = "In order to assure the best level of relaibility"]
pub fn process_sql(sql_action: ProcessSQLSupportedQueries) -> Result<JsonSQLTable, ()> {
    use ProcessSQLSupportedQueries::*;
    match sql_action {
        CreateTable(table_name, columns) => {
            if columns.len() > 0 { // can be treat as boilerplate but i feel safier with this statement
                let mut ready_columns: Vec<JsonSQLTableColumn> = vec![];

                // determine columns and add its to "columns" = vector with all columns for table
                for column in columns {
                    let col_name = column.0;
                    // process specific datatype to appropriate form or maintain other
                    let col_data_type = {
                        match column.1 {
                            SupportedSQLDataTypes::VARCHAR(len) => {
                                // when len = None is used maximum length and in JSON file VARCHAR type has assigned null // specified uinit is always expressed in bytes unit
                                len.map_or_else(
                                    || SupportedSQLDataTypes::VARCHAR(None), // maximum value for varchar is 65535 characters
                                    |val| SupportedSQLDataTypes::VARCHAR(Some(val)),
                                )
                            },
                            _ => column.1,
                        }
                    };
                    // attach constraints
                    let col_constraint = {
                        if column.2.is_some() && column.2.clone().unwrap().len() >= 1 {
                            Some(column.2.unwrap())
                        }
                        else {
                            None
                        }
                    };
                    // compose column type and add it to table columns collection
                    let ready_column = JsonSQLTableColumn {
                        name: col_name,
                        d_type: col_data_type,
                        constraints: col_constraint,
                    };
                    ready_columns.push(ready_column);
                }
                // compose sql table in json format
                let json_sql_table = JsonSQLTable {
                    name: table_name.into(),
                    columns: ready_columns,
                    rows: None,
                };
                // attach computed json table from sql to returned value from whole function
                Ok(json_sql_table)
            }
            else {
                Err(())
            }
        }
        Insert(table_path, columns, rows) => {
            // TODO: Add support for When column type is different then this inffered for query collumn but format of value should be supported like between: "Varchar" and "TEXT" type
            // TODO: Add support for columns operation
            // TODO: Add support for constraints (e.g: When column has got NOT NULL then it must have got assigned value durning INSERT operation)
            // TODO: Add support for autoindexing

            // To perform operation must be minimum one row with inserted data
            if rows.len() > 0 {
                // Obtain already existsing table data (if it exists and is benath correct json format)
                let table_str = if let Ok(data) = fs::read_to_string(table_path) {
                    data
                } else {
                    return Err(());
                };
                let mut table_json =
                    if let Ok(json_table) = serde_json::from_str::<JsonSQLTable>(&table_str) {
                        json_table
                    } else {
                        return Err(());
                    };

                // Attach to table operation
                let db_table_columns = &table_json.columns;
                let db_table_rows = &mut table_json.rows;

                let mut ready_rows = Vec::new() as Vec<Vec<JsonSQLTableColumnRow>>;

                // Iterate over each row with data to insert into table columns
                for row in rows {
                    // Without specified "columns" property number of columns in row must be equal to database column list
                    if db_table_columns.len() == row.len() {
                        // Collection with ready values to insert into table with rows. TODO: Must be checked when insert operation is processing for specific columns on angle of correct with constarints
                        let mut ready_row_values = Vec::new() as Vec<JsonSQLTableColumnRow>;

                        // Iterate over values to insert from one row to insert
                        let row_len = row.len();
                        let mut it_num: usize = 0;
                        while it_num < row_len {
                            let row_value = &row[it_num];
                            let column_for_row_value = &db_table_columns[it_num];

                            // IMPORTANT: Check types correcteness ... type must be the same as column type // + add to match!() all datatype enum tuple memebers
                            if column_for_row_value.d_type == row_value.1
                                || matches!(
                                    column_for_row_value.d_type,
                                    SupportedSQLDataTypes::VARCHAR(_)
                                )
                            {
                                // Additional more sophisticated type checker for more complicated types
                                // Initialy it is always "true" so operation can be performed but in moment when type isn't correct that is changing to "false"
                                let mut allow_to_add = true;
                                // ... more advance checking on column datatype constraints (not same as normal constraints)
                                match column_for_row_value.d_type {
                                    SupportedSQLDataTypes::VARCHAR(column_t_maxlen) => {
                                        // ... check attached value from row to column datatype
                                        match row_value.1 {
                                            SupportedSQLDataTypes::VARCHAR(_) => {
                                                // now always VARCHAR None
                                                // In attached varchar type, value can't be heighter then column varchar length requirements (existing when "column_t_maxlen" is not "None")
                                                if column_t_maxlen.is_some()
                                                    && (column_t_maxlen.unwrap()
                                                        < row_value.0.len() as u16
                                                        || row_value.0.len() as u16 > 65_535)
                                                {
                                                    // TODO: attach on table creation that value must has got smaller length than 65_535 characters for VARCHAR datatype + attach to recognize type from query that after when string has got more then 65_535 charcters then it is no VARCHAR not TEXT (which can has got up to 16_777_215 characters)
                                                    allow_to_add = false;
                                                };
                                            }
                                            _ => (),
                                        }
                                    }
                                    _ => (), // for non-special requirements
                                };

                                // Create ready to insert, to table value for column
                                // Insert only when attached value has got type correct with column datatype
                                if allow_to_add {
                                    let new_value = JsonSQLTableColumnRow {
                                        col: column_for_row_value.name.clone(),
                                        value: row_value.0.clone(),
                                    };
                                    ready_row_values.push(new_value);
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            };

                            it_num += 1;
                        }

                        // Attach row to all rows list
                        ready_rows.push(ready_row_values);
                    } else {
                        break;
                    };
                }

                // Check correcteness and assign values to table "rows" key
                if db_table_rows.is_some()
                    && ready_rows.len() > 0
                    && ready_rows[0].len() == db_table_columns.len()
                {
                    // When already table has got saved rows
                    // ... assign to table new rows
                    let mut db_table_rows = db_table_rows.as_mut().unwrap();
                    db_table_rows.extend(ready_rows);

                    //... assign to table new rows
                    table_json.rows = Some(db_table_rows.clone()); // .clone() becuase i would like get rid of reference without thief whole value

                    //... return table in json format as a result of `INSERT` operation + stop loop
                    return Ok(table_json);
                } else if db_table_rows.is_none() && ready_rows.len() > 0 {
                    // When table hasn't got already any saved rows
                    //... assign to table new rows
                    table_json.rows = Some(ready_rows);

                    //... return table in json format as a result of `INSERT` operation + stop loop
                    return Ok(table_json);
                }

                return Err(()); // otherwise (but not used)
            } else {
                return Err(());
            }
        }
    }
}

#[test]
fn test_process_sql() {
    let tab_name = "new_table".to_string();
    let row1 = (String::from("imie"), SupportedSQLDataTypes::VARCHAR(Some(12)), Some(vec![SupportedSQLColumnConstraints::NOT_NULL]));
    let row2 = (String::from("imie"), SupportedSQLDataTypes::INT, None);
    let computed_table = process_sql(ProcessSQLSupportedQueries::CreateTable(tab_name, vec![row1, row2])).unwrap();
    let serialized = serde_json::to_string(&computed_table).unwrap();
    println!("Computed table is:\n{}", serialized)
}
