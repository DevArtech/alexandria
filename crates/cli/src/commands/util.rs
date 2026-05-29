use alexandria_core::Rel;
use anyhow::Result;

pub fn parse_rel_cli(s: &str) -> Result<Rel> {
    Rel::parse(s).map_err(Into::into)
}
