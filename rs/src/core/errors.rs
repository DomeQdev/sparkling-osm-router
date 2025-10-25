use std::io::Error as IoError;
use thiserror::Error;
use xml::reader::Error as XmlError;

#[derive(Error, Debug)]
pub enum GraphError {
    #[error("File IO Error: {0}")]
    FileIO(#[from] IoError),
    #[error("XML Parsing Error: {0}")]
    XmlParsing(#[from] XmlError),
    #[error("PBF Parsing Error: {0}")]
    PbfParsing(#[from] osmpbf::Error),
    #[error("Download Error: {0}")]
    DownloadError(String),
    #[error("JSON Deserialization Error: {0}")]
    JsonError(#[from] serde_json::Error),
    #[error("Overpass API Error: {0}")]
    OverpassError(String),
    #[error("Invalid OSM Data: {0}")]
    InvalidOsmData(String),
    #[error("Graph with ID {0} not found")]
    GraphNotFound(i32),
    #[error("Profile with ID '{0}' not found in graph container")]
    ProfileNotFound(String),
    #[error("Routing Error: {0}")]
    RoutingError(String),
    #[error("Bincode Error: {0}")]
    Bincode(#[from] Box<bincode::ErrorKind>),
}

pub type Result<T> = std::result::Result<T, GraphError>;
