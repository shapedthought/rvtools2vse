use office::{DataType, Range};

use crate::models::errors::MyError;

pub trait MyRange {
    fn get_col_position(&self, col_name: &String) -> Result<usize, MyError>;
}

impl MyRange for Range {
    fn get_col_position(&self, col_name: &String) -> Result<usize, MyError> {
        let pos = self
            .rows()
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
}

pub trait GetStringValue {
    fn get_string_value(&self, item: String) -> Result<String, MyError>;
}

impl GetStringValue for DataType {
    fn get_string_value(&self, item: String) -> Result<String, MyError> {
        match self {
            DataType::String(t) => Ok(t.to_string()),
            _ => Err(MyError::EnumToString(item)),
        }
    }
}

pub trait GetFloatValue {
    fn get_float_value(&self, item: String) -> Result<f64, MyError>;
}

impl GetFloatValue for DataType {
    fn get_float_value(&self, item: String) -> Result<f64, MyError> {
        match self {
            DataType::Float(t) => Ok(*t),
            DataType::Int(t) => Ok(*t as f64),
            _ => Err(MyError::EnumToFloat(item)),
        }
    }
}

