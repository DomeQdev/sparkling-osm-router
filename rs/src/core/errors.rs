use std::io::Error as IoError;
use thiserror::Error;
use xml::reader::Error as XmlError;

#[derive(Error, Debug)]
pub enum GraphError {
    #[error("File IO Error: {0}")]
    FileIO(#[from] IoError),
    #[error("XML Parsing Error: {0}")]
    XmlParsing(#[from] XmlError),
    #[error("Invalid OSM Data: {0}")]
    InvalidOsmData(String),
}

pub type Result<T> = std::result::Result<T, GraphError>;
