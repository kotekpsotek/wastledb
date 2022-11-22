#![allow(unused)]
use std::{fs, path::{Path, PathBuf}, collections::{HashMap, HashSet}, borrow::BorrowMut};

use serde::{self, Deserialize, Serialize};
use sqlparser::{
    self,
    ast::{ColumnOption, ColumnOptionDef, DataType, Statement, Expr, Value as SQLParserValue, BinaryOperator, Assignment},
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

impl JsonSQLTable {
    fn get_column_type(&self, column_name: &String) -> Option<SupportedSQLDataTypes> {
        let mut col_type = None as Option<SupportedSQLDataTypes>;
        
        for column in &self.columns {
            if column.name == *column_name {
                col_type = Some(column.d_type.clone())
            }
        };

        col_type
    }
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
    /// To which column this row belongs to
    pub col: String,
    /// Row value
    pub value: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct ProcessSQLRowField(pub String, pub SupportedSQLDataTypes); // 1. field value, 2. Field data type (only supported datatypes)

type TableName = String;
type TablePath<'x> = &'x PathBuf;
type ColumnName = String;
type ActionOnlyForTheseColumns = Vec<ColumnName>;
type RowsToProcess = Vec<ProcessSQLRowField>; 

#[derive(Debug)]
/// Includes all "INSERT" operation mutations 
pub enum InsertOperations {
    Into,
    Overwrite
}

#[derive(Debug)]
pub enum ProcessSQLSupportedQueries<'x> {
    Insert(
        TablePath<'x>,
        Option<ActionOnlyForTheseColumns>,
        Vec<RowsToProcess>,
        InsertOperations
    ), // 1. Table name, 2. Optional: Insert only for specified here column names, 3. List with rows values (which will be attached), 4. Operation type
    CreateTable(
        TableName,
        Vec<(
            ColumnName,
            SupportedSQLDataTypes,
            Option<Vec<SupportedSQLColumnConstraints>>,
        )>,
    ), // 1. Table name, 2. Vector with table columns and characteristic for each column
    Truncate(TablePath<'x>),
    Select(TablePath<'x>, ActionOnlyForTheseColumns, Option<Expr>), // path to table, 2. return results for specific record tuples can be all, 3. Select only these records
    Delete(TablePath<'x>, Option<Expr>), // Delete whole table records or only specific record
    Update(TablePath<'x>, Vec<Assignment>, Option<Expr>) // 1. TablePath, 2. Assigments, 3. Condition/s
}

#[derive(Debug, Clone)]
/// Describe operation for row
struct RowWhereOperation {
    /// column name
    column: Option<String>,
    /// column value
    value: Option<String>,
    /// type is from "sqlparser" crate
    op: BinaryOperator,
    /// in match operation indicates whether operation has been successfullperformed
    perf: Option<bool>
}

#[derive(Debug, Clone)]
/// Store table row as prepared for actions with conditions
struct RowOperationForm {
    row: Vec<JsonSQLTableColumnRow>,
    id: u128
}

/// Convert Conditions to more readable and oparatable form. Longer: Function which aim is convert Expr::BinOp to RowWhereOperation expression and put it into "operations_for_row" collection to facilitate performant 'WHERE' computing
fn convert_binarop(expr: Expr, converted_list: &mut Vec<RowWhereOperation>) -> Result<(), ()> {
    if let Expr::BinaryOp { left, op, right } = expr { // for parent
        /// To convert expression witch doesn't rollup further to conjuction (And, Or) 
        fn for_value_and_column(op_row_collection: &mut Vec<RowWhereOperation>, right: &Box<Expr>, left: &Box<Expr>, op: &BinaryOperator) -> Result<(), ()> {
            let no_rollup_cond = RowWhereOperation {
                column: { // column "name"
                    match &**left {
                        Expr::Identifier(d) => {
                            Some(d.value.clone())
                        },
                        _ => return Err(()) // incorrect parsed condition
                    }
                },
                op: op.clone(), // operation type like: Eq, NotEq, Less , ...
                value: { // column "value"
                    match &**right {
                        Expr::Identifier(d) => {
                            Some(d.value.clone())
                        },
                        Expr::Value(value) => {
                            match value {
                                SQLParserValue::SingleQuotedString(sval) | SQLParserValue::DoubleQuotedString(sval) => Some(sval.to_owned()),
                                SQLParserValue::Number(num, _) => {
                                    Some(num.clone())
                                },
                                SQLParserValue::Boolean(boolval) => Some(boolval.to_string()),
                                SQLParserValue::Null => Some(String::from("null")),
                                _ => return Err(())
                            }
                        },
                        _ => return Err(()) // incorrect parsed condition
                    }
                },
                perf: None
            };

            // Add condition part to list
            op_row_collection.push(no_rollup_cond);

            // Performed indicator result
            Ok(())
        }
        
        /// Appropriate action to appropriate outcome
        match op {
            BinaryOperator::And | BinaryOperator::Or => { // for multiple blocks // in that case "left" and "right" keys allways represents next "BinaryOp" struct
                // left
                convert_binarop(*left, converted_list)?;

                // Add conjuction
                let conjuction = RowWhereOperation {
                    column: None,
                    value: None,
                    op: op.clone(),
                    perf: None
                };
                converted_list.push(conjuction);

                // right
                convert_binarop(*right, converted_list)?;

                // Result
                Ok(())
            }, 
            _ => for_value_and_column(converted_list, &right, &left, &op) // for row operations   
        }
    }
    else {
        // No-predicted behave
        Err(())
    }
}

// Obtain all actions requied for match conditions such as: column data type, number from row, searched number
type Comparision = (bool, i128);
fn numeric_matches(json_t_data: &JsonSQLTable, col_name: &Option<String>, col_value: &Option<String>, row_vals: &JsonSQLTableColumnRow) -> (Option<SupportedSQLDataTypes>, Comparision, Comparision) {
    let column_type = json_t_data.get_column_type(col_name.as_ref().unwrap());  // column name always should be provided
    let number_is_checker = col_value.as_ref().map_or_else(|| (false, 0), |success| {
        let parse_op = success.parse::<i128>();
        match parse_op {
            Ok(num) => (true, num),
            Err(_) => (false, 0)
        }
    });
    let number_to_check = row_vals.value.as_ref().map_or_else(|| (false, 0), |success| {
        let parse_op = success.parse::<i128>();
        match parse_op {
            Ok(num) => (true, num),
            Err(_) => (false, 0)
        }
    });

    (column_type, number_is_checker, number_to_check)
}

/// Processing attached SQL query and returns its result as "JsonSQLTable" type ready to serialize, to json format thanks to "serde" and "serde_json" crates
/// When something went bad durning analyze or processing sql query then Error without any description is returned
// Note: Polish characters are not supported by sqlparser, so not use them into queries
#[must_use = "In order to assure the best level of relaibility"]
pub fn process_sql(sql_action: ProcessSQLSupportedQueries) -> Result<JsonSQLTable, ()> {
    use ProcessSQLSupportedQueries::*;
    match sql_action { // only operations which require changes/obtain data/mainupulate file content in any manner
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
        Insert(table_path, columns, rows, op_type) => {
            // TODO: Add support for When column type is different then this inffered for query collumn but format of value should be supported like between: "Varchar" and "TEXT" type
            // TODO: Add support for constraints (e.g: When column has got NOT NULL then it must have got assigned value durning INSERT operation)
            // TODO: Add support for autoindexing
            // TODO: Better system to checking types inside this method (number can't be asigned to string)

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

                // When columns to which values should be inserted were attached then check whether addition for specific columns can be performed
                // When columns weren't attached then ignore this code block
                let mut existing_columns_to_perform_list: std::collections::HashMap<String, JsonSQLTableColumn> = HashMap::new();
                let mut existing_columns_to_perform: Vec<String> = vec![];
                let mut columns_not_included_in_query: Vec<&JsonSQLTableColumn> = vec![];
                if columns.is_some() {
                    let columns = columns.clone().unwrap();
                    
                    // check whether all columns given into query exists and put this column into Vector
                    for column_perf_for in &columns {
                        let databse_has_column = db_table_columns
                            .iter()
                            .enumerate()
                            .any(|val| {
                                if &val.1.name == column_perf_for {
                                    true
                                }
                                else {
                                    false
                                }
                            });
                        
                        if databse_has_column {
                            let column = db_table_columns
                                .iter()
                                .find(|col| {
                                    if &col.name == column_perf_for {
                                        return true
                                    };
                                    
                                    false
                                });
                            
                            if let Some(col) = column {
                                existing_columns_to_perform.push(column_perf_for.clone());
                                existing_columns_to_perform_list.insert(column_perf_for.clone(), col.clone());
                            }
                            else {
                                break;
                            }
                        }
                        else {
                            break;
                        };
                    };

                    // Go further only when all columns from query exists in table and below checking has been done to advantage of "perform further"
                    // When all is correct after check then perform further, else return Err(())
                    if existing_columns_to_perform.len() == columns.len() {
                        // Check whether remained column doesn't have constraints "not null"
                            // ... obtain all reamained columns + assign it to scope range variable
                        columns_not_included_in_query = db_table_columns.
                            iter()
                            .filter(|column| {
                                if !existing_columns_to_perform.contains(&column.name) {
                                    true
                                }
                                else {
                                    false
                                }
                            })
                            .collect::<Vec<&JsonSQLTableColumn>>();
                        
                            // ... Check whether all remained columns so (these "not included in query") not inclueded constraint "NOT NULL" (when not includes then result is "true")
                        let all_remained_dn_null = columns_not_included_in_query // indicate whether all columns doesn't have NOT_NULL constraint
                            .iter()
                            .all(|remained_column| {
                                if let Some(constraints_vec) = &remained_column.constraints {
                                    match &remained_column.constraints {
                                        Some(constraints_vec) => {
                                            // When vector is empty that NOT_NULL constraint doesn't exists so return "false"
                                            if constraints_vec.len() > 0 {
                                                // Check whether in vector with constraints is any NOT_NULL constraint (when is return "false" when is that constraint (from this reason "not" operator begin statement!))
                                                constraints_vec
                                                    .iter()
                                                    .any(|constraint| {
                                                        match constraint.clone() {
                                                            SupportedSQLColumnConstraints::NOT_NULL => false,
                                                            _ => true
                                                        }
                                                    })
                                            }
                                            else {
                                                true
                                            }
                                        },
                                        None => true
                                    }
                                }
                                else {
                                    true
                                }
                            });

                        // When some from remained column contains NOT_NULL constraiint then return Err(()) 
                        if !all_remained_dn_null { 
                            return Err(());
                        }
                        // else ... Go further and perform addition
                    }
                    else {
                        return Err(());
                    }
                }

                // Ready rows to insert into table
                let mut ready_rows = Vec::new() as Vec<Vec<JsonSQLTableColumnRow>>;

                // Iterate over each row with data to insert into table columns. Inside among others are checking row type correctensess respect to column type
                for row in rows {
                    // Always no matter upon operation type columns len must be equal to list of values in row 
                    if db_table_columns.len() == row.len() || (columns.is_some() && row.len() == columns.clone().unwrap().len()) {
                        // Collection with ready values to insert into table with rows
                        let mut ready_row_values = Vec::new() as Vec<JsonSQLTableColumnRow>;

                        // Iterate over values to insert from one row to insert
                        let row_len = row.len();
                        let mut it_num: usize = 0;
                        while it_num < row_len {
                            let row_value = &row[it_num];
                            let column_for_row_value = {
                                if columns.is_none() { // column for normal addition
                                    if db_table_columns.len() > it_num {
                                        &db_table_columns[it_num]
                                    }
                                    else {
                                        break;
                                    }
                                }
                                else { // column for addition for specific columns
                                    if existing_columns_to_perform.len() > it_num {
                                        // Because columns len must be equal to len of values in row so always Some(val)
                                        if let Some(val) = existing_columns_to_perform_list.get(&existing_columns_to_perform[it_num]) {
                                            val
                                        }
                                        else {
                                            break;
                                        }
                                    }
                                    else {
                                        break;
                                    }
                                }
                            };

                            // IMPORTANT: Check types correcteness ... type must be the same as column type // + add to match!() all datatype enum tuple memebers
                            if column_for_row_value.d_type == row_value.1
                                || matches!(
                                    column_for_row_value.d_type,
                                    SupportedSQLDataTypes::VARCHAR(_)
                                )
                                || (column_for_row_value.d_type == SupportedSQLDataTypes::TEXT && matches!(row_value.1, SupportedSQLDataTypes::VARCHAR(_))) // "VARCHAR" should also be added to columns with "TEXT" type (because varchat capacity is smaller then TEXT)
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
                                // For whole columns insert: Insert only when attached value has got type correct with column datatype
                                // For insert for specific columns: Insert value for specific column and full fill remained columns with values null
                                if allow_to_add {
                                    // insert normal value
                                    let new_value = JsonSQLTableColumnRow {
                                        col: column_for_row_value.name.clone(),
                                        value: Some(row_value.0.clone()),
                                    };
                                    ready_row_values.push(new_value);

                                    // Add to row values for remained columns with attached "null" as a value
                                    // below instruction ignore type safeguards ("null" -> can be attached to all keys which doesn't have got NOT_NULL constraint)
                                    if it_num == row_len - 1 && columns.is_some() {
                                        let mut remained_row_values = vec![] as Vec<JsonSQLTableColumnRow>;

                                        for colmn_out_from_query in &columns_not_included_in_query {
                                            let remained_row_value = JsonSQLTableColumnRow {
                                                col: colmn_out_from_query.name.to_owned(),
                                                value: None
                                            };
                                            remained_row_values.push(remained_row_value);
                                        };
                                        ready_row_values.extend(remained_row_values);
                                    };
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            };

                            it_num += 1;
                        }

                        // Attach row to all rows list // Always great result
                        ready_rows.push(ready_row_values);
                    } else {
                        break;
                    };
                }

                // Check correcteness and assign values to table "rows" key
                if db_table_rows.is_some()
                    && ready_rows.len() > 0
                    && ready_rows[0].len() == db_table_columns.len()
                    && matches!(op_type, InsertOperations::Into)
                {
                    // When already table has got saved rows
                    // ... assign to table new rows
                    let mut db_table_rows = db_table_rows.as_mut().unwrap();
                    db_table_rows.extend(ready_rows);

                    //... assign to table new rows
                    table_json.rows = Some(db_table_rows.clone()); // .clone() becuase i would like get rid of reference without thief whole value

                    //... return table in json format as a result of `INSERT` operation + stop loop
                    return Ok(table_json);
                } else if ready_rows.len() > 0
                    && ready_rows[0].len() == db_table_columns.len() 
                    && ((db_table_rows.is_none() && ready_rows.len() > 0) 
                        || (matches!(op_type, InsertOperations::Overwrite) && ready_rows.len() > 0)) 
                {
                    println!("i");
                    // When table hasn't got already any saved rows or INSERT OPERATION has been characterized as "INSERT OVERWRITE TABLE"
                    //... assign to table new rows
                    table_json.rows = Some(ready_rows);

                    //... return table in json format as a result of `INSERT` operation + stop loop
                    return Ok(table_json);
                }

                return Err(()); // otherwise (is returned for example when "ready_rows.len() != db_table_columns.len()" which occurs when row type isn't equal to type specified for column)
            } else {
                return Err(());
            }
        },
        Truncate(table_path) => {
            // To perform whole operation: specified table must exists, table must be in JSON format before serialization. Else "Err(())" is returned
            // Check whether path exists isn't perform here!
            if let Ok(table_str) = fs::read_to_string(table_path) {
                let table_json = serde_json::from_str::<JsonSQLTable>(&table_str);
                if table_json.is_ok() {
                    let mut table_json = table_json.unwrap();
                    table_json.rows = None;
                    
                    Ok(table_json)
                }
                else {
                    Err(())
                }
            }
            else {
                Err(())
            }
        },
        Select(table_path, resulting_columns, conditions) => {
            // When results aren't find then return table rows equal to "None"
            let table_data = fs::read_to_string(table_path).unwrap();
            let mut json_t_data = serde_json::from_str::<JsonSQLTable>(&table_data).unwrap(); // I trust other Database functionalities to maintain correct JSON format
            let mut t_d_rows = json_t_data.clone().rows; // WARNING: for simply access but not for assign values!!!

            if t_d_rows.is_some() {
                    // Attach to each row table unique id
                let mut id_operation_row = 0 as u128;
                let mut t_d_rows = t_d_rows /* prepare each table row for search match operation */
                    .as_mut()
                    .unwrap()
                    .iter()
                    .map(|each_row| {
                        id_operation_row += 1;
                        RowOperationForm {
                            row: each_row.to_owned(),
                            id: id_operation_row
                        }
                    })
                    .collect::<Vec<_>>();
                let mut matched_rows: Vec<Vec<JsonSQLTableColumnRow>> = Vec::new(); // match results are storing here

                    // ... Table must have some rows to go further
                if t_d_rows.len() > 0 {
                    // Get whether user pass columns which are into table or pass "all" option (for return all columns)
                    let table_col_names = json_t_data.columns.iter()
                        .enumerate()
                        .filter_map(|col| {
                          Some(&col.1.name)
                       })
                        .collect::<Vec<&String>>();
                    // Whether user add table column names or appropraite option
                    let user_pass_table_cols = resulting_columns.iter()
                        .enumerate()
                        .all(|col_to_ret| {
                            let col_name = col_to_ret.1;
                        
                            if table_col_names.contains(&col_name) || col_name == &"all".to_string() {
                                return true;
                            };

                            false
                        });

                    //... Search results using conditions from 'WHERE'
                    if let Some(expr_conditions) = conditions {
                        // list with converted expressions from 'WHERE'
                        let mut operations_for_row: Vec<RowWhereOperation> = Vec::new(); // [{ column: Some("gender"), value: Some("male"), op: Eq }, { op: And, column: None, value: None }]

                        // Convert whole to expected form
                        convert_binarop(expr_conditions, &mut operations_for_row)?;

                        let mut s_rows = Vec::new() as Vec<RowOperationForm>; // matched rows storage // are storing in this scope and later are "send" to heighter scope
                        let mut op_performed_whole = true; // when false result shoudn't be returned and search operation performed further
                        let mut seeked_rows: Vec<u128> = Vec::new(); // Vector with ids rows which has been matched

                        // Iterate over conditions and try to find appropriate columns
                        type RowId = u128;
                        let mut and_outcomes_for_all_conditions: Vec<Vec<(bool, RowId)>> = Vec::new(); // matches are in order in that conditions has been passed
                        let mut or_exception_for_and_conditions: HashSet<RowId> = HashSet::new(); // row ids list which has been match by "OR" conjuction conditions thus shoudn't be deleted by "AND" delete row proceduree
                        let mut it_op_id = 0;
                        loop {
                            if it_op_id < operations_for_row.len() && op_performed_whole {
                                // get condition to later match
                                let rm = operations_for_row.clone(); // to easy compare in And, Or conditions
                                let op_for_row = &mut operations_for_row[it_op_id];
                                // println!("{:#?}", op_for_row);

                                // Src operation trashold:
                                let sc_name = op_for_row.column.clone();
                                let sc_val = op_for_row.value.clone();
                                let mut match_found: bool = false;
    
                                //... Comparing clousure // op: "Eq"/"Less" etc...
                                let mut search_match_in_row = |op_for_row: &mut RowWhereOperation| {
                                    let mut and_condition_outcomes_from_each_row: Vec<(bool, RowId)> = Vec::new(); // store row matches for this condition (selected condition by "loop" block)

                                    // Search match in each "row" of table
                                    for row in &*t_d_rows {
                                        let mut and_con_match_found_in_row = false;
                                        
                                        // Search result in each "row value" from table
                                        for row_vals in &row.row {
                                            // For matched results: perform all that sugest that row has been match
                                            // KEEP Vigilance: The most promiment function for whole search operation
                                            fn match_success(match_found: &mut bool, and_con_match_found_in_row: &mut bool, op_for_row: &mut RowWhereOperation, s_rows: &mut Vec<RowOperationForm>, matched_rows_list: &mut Vec<u128>, row: &RowOperationForm) {
                                                // Pointers
                                                *and_con_match_found_in_row = true; // that found result in row
                                                *match_found = true; // match is here so indicate other members about that
                                                op_for_row.perf = Some(true); // indicate that operation has been successfull performed
                                                
                                                // Body
                                                if !matched_rows_list.contains(&row.id) { // Attach result only when it was not found previous
                                                    s_rows.push(row.clone()); // attach seeked row to seeked rows list
                                                    matched_rows_list.push(row.id); // attach row id to matched rows list
                                                }
                                            }

                                            // Perform specific action abd add positive match result to results list
                                            match op_for_row.op {
                                                BinaryOperator::Eq => { // values must be equal
                                                    if &row_vals.col == sc_name.as_ref().unwrap() && row_vals.value == sc_val {
                                                        match_success(&mut match_found, &mut and_con_match_found_in_row, &mut op_for_row.clone(), &mut s_rows, &mut seeked_rows, row);
                                                        break;
                                                    }
                                                },
                                                BinaryOperator::NotEq => {
                                                    if &row_vals.col == sc_name.as_ref().unwrap() && row_vals.value != sc_val {
                                                        match_success(&mut match_found, &mut and_con_match_found_in_row, &mut op_for_row.clone(), &mut s_rows, &mut seeked_rows, row);
                                                        break;
                                                    }
                                                },
                                                BinaryOperator::Gt => { // value from database must be greater then given
                                                    let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &sc_name, &sc_val, &row_vals); // data required for all numeric operations

                                                    if (&row_vals.col == sc_name.as_ref().unwrap()) && (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                        if number_to_check.1 > number_is_checker.1 { // value from row must be greater then that from query
                                                            match_success(&mut match_found, &mut and_con_match_found_in_row, op_for_row, &mut s_rows, &mut seeked_rows, row);
                                                            break;
                                                        }
                                                    }
                                                },
                                                BinaryOperator::GtEq => {
                                                    let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &sc_name, &sc_val, &row_vals); // data required for all numeric operations

                                                    if (&row_vals.col == sc_name.as_ref().unwrap()) && (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                        if number_to_check.1 >= number_is_checker.1 { // value from row must be greater or equal to that from query
                                                            match_success(&mut match_found, &mut and_con_match_found_in_row, op_for_row, &mut s_rows, &mut seeked_rows, row);
                                                            break;
                                                        }
                                                    }
                                                },
                                                BinaryOperator::Lt => {
                                                    let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &sc_name, &sc_val, &row_vals); // data required for all numeric operations

                                                    if (&row_vals.col == sc_name.as_ref().unwrap()) && (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                        if number_to_check.1 < number_is_checker.1 { // value from row must be greater or equal to that from query
                                                            match_success(&mut match_found, &mut and_con_match_found_in_row, op_for_row, &mut s_rows, &mut seeked_rows, row);
                                                            break;
                                                        }
                                                    }
                                                },
                                                BinaryOperator::LtEq => {
                                                    let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &sc_name, &sc_val, &row_vals); // data required for all numeric operations

                                                    if (&row_vals.col == sc_name.as_ref().unwrap()) && (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                        if number_to_check.1 <= number_is_checker.1 { // value from row must be greater or equal to that from query
                                                            match_success(&mut match_found, &mut and_con_match_found_in_row, op_for_row, &mut s_rows, &mut seeked_rows, row);
                                                            break;
                                                        }
                                                    }
                                                }
                                                _ => () // no handled
                                            }
                                        }
                                   
                                        // Add result from condition search from this row
                                        and_condition_outcomes_from_each_row.push((and_con_match_found_in_row, row.id));

                                        // Add match searched by "OR" condition to not remove by "AND" condition procedure list
                                        if match_found && it_op_id != 0 && rm[it_op_id - 1].op == BinaryOperator::Or {
                                            or_exception_for_and_conditions.insert(row.id);
                                        };
                                    }

                                    // Add this ("loop" block iteration) condition search result from each row to list search outcomes for each condition
                                        // .. Don't add and condition for Or conjuction operator which preceedes condition or are behind first condition
                                    if (it_op_id == 0 && rm.len() > 1 && rm[1].op == BinaryOperator::And) || (it_op_id > 0 && rm[it_op_id - 1].op != BinaryOperator::Or) || ((it_op_id > 0 && rm[it_op_id - 1].op == BinaryOperator::And) || (rm.len() > it_op_id + 1 && rm[it_op_id + 1].op == BinaryOperator::And)) {
                                        and_outcomes_for_all_conditions.push(and_condition_outcomes_from_each_row);
                                    };
                                    
                                    // For consistancy: when match wasn't found (cohestive working should be represented in this way)
                                    if !match_found {
                                        op_for_row.perf = Some(false);
                                    }
                                };
    
                                // And/Or conditions pissed here
                                if let BinaryOperator::And = op_for_row.op {
                                    ()
                                }
                                else if let BinaryOperator::Or = op_for_row.op {
                                    () // Do nothing... Just go to next iteration (no "continue" statement (because it stop iterations counting hence next iteration will be reffering to this cycle so this made infinite loop))
                                }
                                else {
                                    // Lead further for other "op" types like Eq/Gt/Less etc...
                                    search_match_in_row(op_for_row);

                                    // When result hasn't been matched in any row by above clousure (enclosed in brackets "{}")
                                    if !match_found {
                                        // Specific use cases
                                        if it_op_id > 0 && rm[it_op_id - 1].op == BinaryOperator::And && (rm.len() > it_op_id + 1 && rm[it_op_id + 1].op == BinaryOperator::Or) {
                                            // When match hasn't been found for "AND" conjcution but next conjuction is "OR"
                                            ()
                                        }
                                    };
                                }

                                // increase iterated elements colunt
                                it_op_id += 1;
                            }
                            else {
                                break;
                            }
                        };
                        // println!("{:?}", and_outcomes_for_all_conditions);

                        // Remove matches (rows) which aren't fullfilled for all and conditions
                        let and_conditions_len = and_outcomes_for_all_conditions.len();
                        if and_conditions_len > 0 {
                                // Iterate on only row outcomes for first condition because all conditions must have always same rows count and our aim is to obtain only each row id for code located inside
                            for f_cond_row in and_outcomes_for_all_conditions[0].iter() {
                                    //.. Whether in all "AND" conditions this row has been matched
                                let matches_for_row = and_outcomes_for_all_conditions
                                    .iter()
                                    .all(|ocondition| {
                                        let mut match_found_for_condition: bool = false;
                                        for (result, id) in ocondition {
                                            if *id == f_cond_row.1 {
                                                match_found_for_condition = *result;
                                            }
                                        };
                                        match_found_for_condition
                                    });
                                    // Remove row which doesn't match for all "AND" conditions and when row id isn't on "OR" exceptions list
                                if !matches_for_row {
                                    // println!("Didn't found match for row: {}\nOr exceptions list: {:?}", f_cond_row.1, or_exception_for_and_conditions);
                                    for (on_list_id, RowOperationForm { row: _, id: row_id }) in s_rows.clone().iter().enumerate() {
                                        // When row hasn't been matched, it isn't on or exceptions list and doesn't matched row id is equal to iterated row id on list with seeked rows
                                        if *row_id == f_cond_row.1 && !or_exception_for_and_conditions.contains(&row_id) {
                                            s_rows.remove(on_list_id);
                                        }
                                    }
                                }
                            }
                        }

                        // Attach search results to next processing stage (only when where results has been found else return table without rows as a result of function)
                        if s_rows.len() > 0 {
                            std::mem::drop(t_d_rows); // remove prepared rows from memory faster than RAII should do this automatically
                            matched_rows.extend(
                                s_rows
                                        .iter()
                                        .map(|RowOperationForm { row, id }| {
                                            row.clone()
                                        })
                                        .collect::<Vec<_>>()
                            );
                        }
                        else {
                            json_t_data.rows = None;
                            return Ok(json_t_data);
                        };
                    }
                    // println!("{:#?}", matched_rows);

                    // Go ahead only when user pass table column names or "all" option
                    // Send to user only specified by him columns only when user pass these columns
                    if user_pass_table_cols {
                        // Return only fields for columns which user would like to get
                        if resulting_columns[0] == "all" {
                            // Return all columns for matched records
                            json_t_data.rows = Some(matched_rows);
                            return Ok(json_t_data);
                        }
                        else {
                            // Return only fields for columns which user would like to get 
                            let mut f_results = vec![] as Vec<Vec<JsonSQLTableColumnRow>>;
                            for row in matched_rows {
                                let mut row_passed_fields_ready = vec![] as Vec<JsonSQLTableColumnRow>;
                                let _ = row
                                    .iter()
                                    .enumerate()
                                    .filter(|field| {
                                        let f_d = field.1;

                                        if resulting_columns.contains(&f_d.col) {
                                            return true
                                        };

                                        return false
                                    })
                                    .collect::<Vec<(usize, &JsonSQLTableColumnRow)>>()
                                    .into_iter()
                                    .for_each(|record| {
                                        row_passed_fields_ready.push(record.1.to_owned())
                                    });
                                f_results.push(row_passed_fields_ready)
                            }

                            json_t_data.rows = Some(f_results);
                            return Ok(json_t_data);
                        }
                    }
                    else {
                        return Err(())
                    }
                }
                else {
                    // Return table withput rows // with null benath "rows" key
                    json_t_data.rows = None; // Return null for "rows" but not empty array. "serde_json" threat that as null in json file
                    return Ok(json_t_data)
                }
            }
            else {
                // Return table withput rows // with null benath "rows" key
                return Ok(json_t_data)
            }
        },
        Delete(table_path, condition) => {
            // Delete matched rows from table and return deleted rows
            let table_data = fs::read_to_string(table_path).unwrap();
            let mut json_t_data = serde_json::from_str::<JsonSQLTable>(&table_data).unwrap();

            fn save_updated_table(table: JsonSQLTable, path: &PathBuf) -> Result<(), ()> {
                let s = serde_json::to_string(&table).unwrap();
                fs::write(path, s).map_or_else(|_| Err(()), |_| Ok(()))
            }

            // To peroform delete operation table must have got some rows otherwise will be return table without any rows
            if let Some(rows_set) = &json_t_data.rows {
                match condition {
                    Some(condition_body) => {
                        // list with converted expressions from 'WHERE'
                        let mut operations_for_row: Vec<RowWhereOperation> = Vec::new(); // [{ column: Some("gender"), value: Some("male"), op: Eq }, { op: And, column: None, value: None }]                   
                        let mut prep_rows = rows_set.into_iter()
                            .enumerate()
                            .map(|val| {
                                RowOperationForm {
                                    id: val.0 as u128,
                                    row: val.1.to_owned()
                                }
                            })
                            .collect::<Vec<_>>();
                        // Convert whole to expected form
                        convert_binarop(condition_body, &mut operations_for_row)?;
                        let mut rows_to_delete: HashSet<u128> = HashSet::new(); // store only unique identificators for rows
                        // iterate over conditions and search matches
                        for cond in operations_for_row {
                            if cond.op != BinaryOperator::And && cond.op != BinaryOperator::Or {
                                let cond_colname = cond.column.unwrap();
                                let cond_colvalue = cond.value.unwrap();
                                for prep_row in &prep_rows {
                                    for row in &prep_row.row {
                                        let row_column = row.col.clone();
                                        let row_value = row.value.clone().unwrap(); // FIXME: Null will cause error here!!!
                                        if row_column == cond_colname.clone() {
                                            let mut when_success_in_match = || {
                                                rows_to_delete.insert(prep_row.id);
                                            };
    
                                            // Perform specific action and 
                                            match cond.op {
                                                BinaryOperator::Eq => { // values must be equal
                                                    if row_value == cond_colvalue {
                                                        when_success_in_match();
                                                    }
                                                },
                                                BinaryOperator::NotEq => {
                                                    if row_value != cond_colvalue.clone() {
                                                        when_success_in_match();
                                                    }
                                                },
                                                BinaryOperator::Gt => { // value from database must be greater then given
                                                    let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &Some(cond_colname.to_owned()), &Some(cond_colvalue.to_owned()), row); // data required for all numeric operations
    
                                                    if (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                        if number_to_check.1 > number_is_checker.1 {
                                                            when_success_in_match()
                                                        }
                                                    }
                                                },
                                                BinaryOperator::GtEq => {
                                                    let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &Some(cond_colname.to_owned()), &Some(cond_colvalue.to_owned()), row); // data required for all numeric operations
    
                                                    if (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                        if number_to_check.1 >= number_is_checker.1 {
                                                            when_success_in_match()
                                                        }
                                                    }
                                                },
                                                BinaryOperator::Lt => {
                                                    let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &Some(cond_colname.to_owned()), &Some(cond_colvalue.to_owned()), row); // data required for all numeric operations
    
                                                    if (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                        if number_to_check.1 < number_is_checker.1 {
                                                            when_success_in_match()
                                                        }
                                                    }
                                                },
                                                BinaryOperator::LtEq => {
                                                    let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &Some(cond_colname.to_owned()), &Some(cond_colvalue.to_owned()), row); // data required for all numeric operations
    
                                                    if (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                        if number_to_check.1 <= number_is_checker.1 {
                                                            when_success_in_match()
                                                        }
                                                    }
                                                }
                                                _ => () // no handled
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Delete rows which matches to query and add them to list collected deleted rows
                        let mut deleted_rows: Vec<Vec<JsonSQLTableColumnRow>> = Vec::new(); // list with deleted rows
                        for row_id_to_delete in rows_to_delete {
                            prep_rows.clone().into_iter().enumerate().for_each(|prep_orow| {
                                if prep_orow.1.id == row_id_to_delete {
                                    deleted_rows.push(prep_orow.1.row);
                                    prep_rows.remove(prep_orow.0);
                                };
                            })
                        };
                        // Save table without deleted rows
                        let table_content = serde_json::to_string::<JsonSQLTable>(&JsonSQLTable { 
                            rows: {
                                let prep = prep_rows.into_iter()
                                    .map(|row| row.row)
                                    .collect::<Vec<_>>();
                                if prep.len() > 0 {
                                    Some(prep)
                                }
                                else {
                                    None
                                }
                            }, 
                            ..json_t_data.clone()
                        }).unwrap(); // Assumes that table content always has got correct JSON syntax
                        fs::write(table_path, table_content);
                        // Return table with deleted rows
                        json_t_data.rows = Some(deleted_rows);
                        return Ok(json_t_data)
                    },
                    None => {
                        // Delete all rows from table
                        let table_before_rows_deletion = json_t_data.clone();
                        json_t_data.rows = None;
                        save_updated_table(json_t_data, table_path)?;
                        Ok(table_before_rows_deletion)
                    }
                }
            }
            else {
                Ok(json_t_data)
            }
        },
        Update(table_path, assigments, condition) => {
            // Update table rows and return table which have got updated rows
            let table_data = fs::read_to_string(table_path).unwrap();
            let mut json_t_data = serde_json::from_str::<JsonSQLTable>(&table_data).unwrap();

            // To peroform "update" operation table must have got some rows otherwise will be return table without any rows hence without performed update operation
            if matches!(json_t_data.rows, Some(_)) {
                    // Function to get Assigment value content and assigment value type
                let get_asg_value = |value| {
                    let asg_value = {
                        if let Expr::Value(data_type) = value {
                            use sqlparser::ast::Value;
                            match data_type {
                                Value::Boolean(bool) => Some((bool.to_string(), SupportedSQLDataTypes::BOOLEAN)),
                                Value::Number(num, _) => Some((num.to_owned(), SupportedSQLDataTypes::INT)),
                                Value::DoubleQuotedString(stri) | Value::EscapedStringLiteral(stri) | Value::SingleQuotedString(stri) => {
                                    let stri_data_type = if stri.len() > 65_535 {
                                        SupportedSQLDataTypes::TEXT
                                    }
                                    else {
                                        SupportedSQLDataTypes::VARCHAR(None)
                                    };
                                    Some((stri.to_owned(), stri_data_type))
                                },
                                _ => None
                            }
                        }
                        else {
                            None
                        }
                    };

                    asg_value
                };
                    // Function checking whether assigment inserted value and its type is correct with table column data type
                let type_checker: fn(SupportedSQLDataTypes, &SupportedSQLDataTypes, &String) -> bool = |table_column_type, assigment_type, assigment_value| {
                    if table_column_type == *assigment_type
                    || ((matches!(table_column_type, SupportedSQLDataTypes::VARCHAR(_)) || table_column_type == SupportedSQLDataTypes::TEXT) && *assigment_type == SupportedSQLDataTypes::INT)
                    || (matches!(table_column_type, SupportedSQLDataTypes::VARCHAR(_)) && matches!(*assigment_type, SupportedSQLDataTypes::VARCHAR(_))) {
                        match table_column_type {
                            SupportedSQLDataTypes::VARCHAR(length) => {
                                let length = {
                                    if let Some(len) = length {
                                        len
                                    }
                                    else {
                                        u16::MAX
                                    }
                                };
                            
                                // Value inserted to sting column with type Varchar must be smaller then inserted length or pre-defined for u16 type so 65_535
                                if length as usize >= assigment_value.len() {
                                    true
                                }
                                else {
                                    false
                                }
                            },
                            SupportedSQLDataTypes::TEXT => {
                                if assigment_value.len() <= 16_777_215 {
                                    true
                                }
                                else {
                                    false
                                }
                            },
                            _ => true
                        }
                    }
                    else {
                        false
                    }
                };

                // When conditions has been passed update only specific row otheriwse update all rows
                if condition.is_some() {
                    // Update only specific rows (matched by condition from 'WHERE' statement)
                    let table_rows = json_t_data.rows.clone().unwrap();
                    let table_columns_names = json_t_data.columns.clone().into_iter().map(|column| column.name).collect::<Vec<_>>();
                    let mut prep_rows = table_rows.into_iter()
                        .enumerate()
                        .map(|val| {
                            RowOperationForm {
                                id: val.0 as u128,
                                row: val.1.to_owned()
                            }
                        })
                        .collect::<Vec<_>>();
    
                    // Convert condition to more redable form
                    let mut operations_for_row: Vec<RowWhereOperation> = Vec::new(); // [{ column: Some("gender"), value: Some("male"), op: Eq }, { op: And, column: None, value: None }]                   
                    convert_binarop(condition.unwrap(), &mut operations_for_row)?;
    
                    // iterate over conditions and search matches
                    let mut cond_rows_matched_ids: HashSet<u128> = HashSet::new();                
                    for cond in operations_for_row {
                        if cond.op != BinaryOperator::And && cond.op != BinaryOperator::Or {
                            let cond_colname = cond.column.unwrap();
                            let cond_colvalue = cond.value.unwrap();
                            for prep_row in &prep_rows {
                                for row in &prep_row.row {
                                    let row_column = row.col.clone();
                                    let row_value = row.value.clone().unwrap(); // FIXME: Null will cause error here!!!
                                    if row_column == cond_colname.clone() {
                                        let mut when_success_in_match = || {
                                            cond_rows_matched_ids.insert(prep_row.id);
                                        };
    
                                        // Perform specific action and 
                                        match cond.op {
                                            BinaryOperator::Eq => { // values must be equal
                                                if row_value == cond_colvalue {
                                                    when_success_in_match();
                                                }
                                            },
                                            BinaryOperator::NotEq => {
                                                if row_value != cond_colvalue.clone() {
                                                    when_success_in_match();
                                                }
                                            },
                                            BinaryOperator::Gt => { // value from database must be greater then given
                                                let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &Some(cond_colname.to_owned()), &Some(cond_colvalue.to_owned()), row); // data required for all numeric operations
    
                                                if (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                    if number_to_check.1 > number_is_checker.1 {
                                                        when_success_in_match()
                                                    }
                                                }
                                            },
                                            BinaryOperator::GtEq => {
                                                let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &Some(cond_colname.to_owned()), &Some(cond_colvalue.to_owned()), row); // data required for all numeric operations
    
                                                if (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                    if number_to_check.1 >= number_is_checker.1 {
                                                        when_success_in_match()
                                                    }
                                                }
                                            },
                                            BinaryOperator::Lt => {
                                                let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &Some(cond_colname.to_owned()), &Some(cond_colvalue.to_owned()), row); // data required for all numeric operations
    
                                                if (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                    if number_to_check.1 < number_is_checker.1 {
                                                        when_success_in_match()
                                                    }
                                                }
                                            },
                                            BinaryOperator::LtEq => {
                                                let (column_type, number_is_checker, number_to_check) = numeric_matches(&json_t_data, &Some(cond_colname.to_owned()), &Some(cond_colvalue.to_owned()), row); // data required for all numeric operations
    
                                                if (column_type.is_some() && column_type.unwrap() == SupportedSQLDataTypes::INT) && (number_is_checker.0 && number_to_check.0) {
                                                    if number_to_check.1 <= number_is_checker.1 {
                                                        when_success_in_match()
                                                    }
                                                }
                                            }
                                            _ => () // no handled
                                        }
                                    }
                                }
                            }
                        }
                    }
    
                    // Search matched rows to conditions and change their values depends on "assigments" 
                    let mut performed_updation = false;
                    for matched_row_id in cond_rows_matched_ids.iter() {
                        let matched_row = prep_rows.clone().into_iter().enumerate().find(|row_prep| {
                            if row_prep.1.id == *matched_row_id {
                                return true
                            };
                            false
                        });
    
                        // Match row signature / whether found row or not
                        match matched_row {
                            Some((id_on_list_match, RowOperationForm { row, id: _ })) => {
                                // Iterate over columns from matched row
                                for (row_value_id, JsonSQLTableColumnRow { col: table_column_name, value }) in row.into_iter().enumerate() {
                                    // Iterate over assigments in order to find column which must be changed
                                    for Assignment { id: as_column_obj, value } in &assigments {
                                        // When column name from iterated assigment is same as column name which must change
                                        if &table_column_name == &as_column_obj[0].value {
                                                // ... Obtain value and value datatype from assigment   
                                                // ... Get Assigment value and value type               
                                            match get_asg_value(value.to_owned()) {
                                                Some(asg_value) => {
                                                    // ... Obtain data type from column and check assigned value data type correcteness respect to table column data type 
                                                    let table_column_type = json_t_data.get_column_type(&table_column_name).expect("Unexpected behaviour!");
                                                    if type_checker(table_column_type, &asg_value.1, &asg_value.0) {
                                                        prep_rows[id_on_list_match].row[row_value_id].value = Some(asg_value.0);
                                                        performed_updation = true;
                                                    }
                                                },
                                                None => break
                                            };
                                        }
                                    }
                                }
                            },
                            None => break
                        }
                    };
                    
                    // If condition has been matched and updated then return table with updated data otherwise branchback not updated table
                    // When not updated table was returned that can means that assigment has column names which aren't table columns
                    if performed_updation {
                        Ok(
                            JsonSQLTable { 
                                rows: {
                                    let acceptable_row_fmt = prep_rows.into_iter().map(|row| row.row).collect::<Vec<_>>();
                                    Some(acceptable_row_fmt)
                                },
                                ..json_t_data
                            }
                        )
                    }
                    else {
                        Ok(json_t_data)
                    }
                }
                else {
                    // Update all rows which will be match to rows

                    // Iterate over each table row and his id
                    for table_row in json_t_data.rows.clone().unwrap().into_iter().enumerate() {
                        // Iterate over each table row column and its id in row
                        for table_row_column in table_row.1.into_iter().enumerate() {
                            let table_row_column_name = (table_row_column.1).col;
                            // Iterate over each assigment from 'SET'
                            for Assignment { id: assigment_column_object, value } in &assigments {
                                if table_row_column_name == assigment_column_object[0].value {
                                        // ... Obtain value and value datatype from assigment 
                                        // ... Get Assigment value and value type               
                                    match get_asg_value(value.to_owned()) {
                                        Some(asg_value) => {
                                            // Update row value
                                            let table_column_type = json_t_data.get_column_type(&table_row_column_name).expect("Unexpected behaviour!");
                                            if type_checker(table_column_type, &asg_value.1, &asg_value.0) {
                                                json_t_data.rows.as_mut().unwrap()[table_row.0][table_row_column.0].value = Some(asg_value.0);
                                            };
                                        },
                                        None => break
                                    };
                                };
                            };
                        };
                    };

                    // Return table with or without updated rows (for loop cannot edit any row when appropriate environment variable will be fullfiled)
                    Ok(json_t_data)
                }
            }
            else {
                Ok(json_t_data)
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
