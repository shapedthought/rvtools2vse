use office::DataType;

use crate::models::errors::MyError;

pub fn get_position(data: &office::Range, col_name: &String) -> Result<usize, MyError> {
    let pos = data.rows()
        .next()
        .unwrap()
        .iter()
        .position(|x| x == &DataType::String(col_name.to_string()));
    
    if let Some(p) = pos {
        Ok(p)
    } else {
        Err(MyError::ColumnPosition(col_name.to_string()))
    }
}

pub fn get_string_value(data: &DataType, item: String) -> Result<String, MyError> {
    match data {
        DataType::String(t) => Ok(t.to_string()),
        _ => Err(MyError::EnumToString(item)),
    }
}

pub fn get_float_value(data: &DataType, item: String) -> Result<f64, MyError> {
    match data {
        DataType::Float(t) => Ok(*t),
        DataType::Int(t) => Ok(*t as f64),
        _ => Err(MyError::EnumToFloat(item)),
    }
}