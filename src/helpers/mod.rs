use calamine::{Data, Range};

use crate::models::errors::MyError;

pub trait ColPosition {
    fn get_col_pos(&self, col_name: &String) -> Result<usize, MyError>;
}

impl ColPosition for Range<Data> {
    fn get_col_pos(&self, col_name: &String) -> Result<usize, MyError> {
        let data_type = &self.rows().next().unwrap();

        let pos = data_type
            .iter()
            .position(|x| x == &Data::String(col_name.to_string()));

        if let Some(p) = pos {
            Ok(p)
        } else {
            Err(MyError::ColumnPosition(format!("{}", col_name.to_string())))
        }
    }
}

pub trait GetString {
    fn get_string_value(&self, item: String, row: usize) -> Result<String, MyError>;
}

impl GetString for Data {
    fn get_string_value(&self, item: String, row: usize) -> Result<String, MyError> {
        match self {
            Data::String(t) => Ok(t.to_string()),
            Data::Empty => Ok("None".to_string()),
            _ => Err(MyError::EnumToString(format!(
                "{} - row {} - Datatype {:?}",
                item, row, self
            ))),
        }
    }
}

pub trait GetFloat {
    fn get_float_value(&self, item: String, row: usize) -> Result<f64, MyError>;
}

impl GetFloat for Data {
    fn get_float_value(&self, item: String, row: usize) -> Result<f64, MyError> {
        match self {
            Data::Float(t) => Ok(*t),
            Data::Int(t) => Ok(*t as f64),
            Data::Empty => Ok(0.0),
            _ => Err(MyError::EnumToFloat(format!(
                "{} - row {} - Datatype {:?}",
                item, row, self
            ))),
        }
    }
}

/// Best-effort string for optional columns (Raw, modes) that may be Bool/Float/String.
pub fn as_display_string(cell: &Data) -> String {
    match cell {
        Data::String(t) => t.clone(),
        Data::Bool(b) => b.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => f.to_string(),
        Data::Empty => String::new(),
        _ => String::new(),
    }
}

pub fn is_physical_rdm_mode(raw_comp_mode: &str) -> bool {
    raw_comp_mode.to_ascii_lowercase().contains("physicalmode")
}

pub fn is_independent_disk_mode(disk_mode: &str) -> bool {
    let m = disk_mode
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "");
    m.contains("independent")
}

pub fn is_shared_disk_mode(sharing: &str) -> bool {
    let s = sharing
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "");
    // VMware VirtualDiskSharing::sharingMultiWriter; RVTools "Sharing" / "Sharing mode"
    s.contains("multiwriter") || s == "sharingmultiwriter"
}

pub fn tools_unreliable(tools: Option<&str>) -> bool {
    tools
        .map(|t| t.to_ascii_lowercase())
        .is_some_and(|t| t.contains("toolsnotinstalled") || t.contains("toolsnotrunning"))
}
