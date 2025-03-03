use crate::errors::{GraphError, Result};
use xml::attribute::OwnedAttribute;

pub fn parse_attribute<T: std::str::FromStr>(
    attributes: &[OwnedAttribute],
    attribute_name: &str,
    error_message: &str,
) -> Result<T>
where
    <T as std::str::FromStr>::Err: std::fmt::Debug,
{
    attributes
        .iter()
        .find(|attr| attr.name.local_name == attribute_name)
        .and_then(|attr| attr.value.parse::<T>().ok())
        .ok_or_else(|| GraphError::InvalidOsmData(error_message.to_string()))
}
