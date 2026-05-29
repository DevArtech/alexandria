//! The memory's structural "table of contents": the collections and tags it is
//! organized by, with counts. Lets an agent orient itself and scope retrieval to
//! the right facets for any domain instead of relying on fuzzy matching alone.

use serde::Serialize;

use crate::error::Result;
use crate::index::Index;

#[derive(Debug, Clone, Serialize)]
pub struct FacetCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Catalog {
    pub total_engrams: usize,
    pub collections: Vec<FacetCount>,
    pub tags: Vec<FacetCount>,
}

/// Build the catalog of available collections and tags (relational tier excluded).
pub fn catalog(index: &Index) -> Result<Catalog> {
    let collections = index
        .list_collections()?
        .into_iter()
        .map(|(name, count)| FacetCount { name, count })
        .collect();
    let tags = index
        .list_tags()?
        .into_iter()
        .map(|(name, count)| FacetCount { name, count })
        .collect();
    Ok(Catalog {
        total_engrams: index.count_engrams()?,
        collections,
        tags,
    })
}
