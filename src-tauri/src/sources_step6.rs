//! Step 6: read-only inspection of provider staging. No changes to jobs/reviews.
use rusqlite::{Connection, params};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct ExclusionReason {
    pub reason: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct SourceMetrics {
    pub source: String,
    pub stored_listings: i64,
    pub eligible: i64,
    pub unverified: i64,
    pub excluded: i64,
    pub unclassified: i64,
    pub descriptions_available: i64,
    pub short_descriptions: i64,
    pub unknown_publication_dates: i64,
    pub unknown_workloads: i64,
    pub stale_30_days: i64,
    pub revision_snapshots: i64,
    pub last_retrieved_at: Option<String>,
    pub last_published_at: Option<String>,
    pub exclusion_reasons: Vec<ExclusionReason>,
}

pub fn inspect(db: &Connection) -> Result<Vec<SourceMetrics>, String> {
    // This function intentionally does NOT migrate or write to the database.
    // A missing table is a genuine initialization issue, not an empty dataset.
    let mut stmt = db.prepare(r#"
        SELECT s.source,
               COUNT(*),
               COALESCE(SUM(CASE WHEN e.classification='eligible' THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN e.classification='unverified' THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN e.classification='excluded' THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN e.classification IS NULL THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN LENGTH(TRIM(COALESCE(s.description,'')))>0 THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN LENGTH(TRIM(COALESCE(s.description,''))) BETWEEN 1 AND 149 THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN s.published_at IS NULL OR TRIM(s.published_at)='' THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN s.grades_json IS NULL OR TRIM(s.grades_json) IN ('','[]','null') THEN 1 ELSE 0 END),0),
               COALESCE(SUM(CASE WHEN datetime(s.retrieved_at)<datetime('now','-30 days') THEN 1 ELSE 0 END),0),
               MAX(s.retrieved_at), MAX(s.published_at)
        FROM source_listings s
        LEFT JOIN source_eligibility e ON e.source=s.source AND e.source_id=s.source_id
        GROUP BY s.source
        ORDER BY s.source
    "#).map_err(|e| format!("Source audit query: {e}"))?;
    let rows = stmt.query_map([], |r| {
        Ok(SourceMetrics {
            source:r.get(0)?, stored_listings:r.get(1)?, eligible:r.get(2)?,
            unverified:r.get(3)?, excluded:r.get(4)?, unclassified:r.get(5)?,
            descriptions_available:r.get(6)?, short_descriptions:r.get(7)?,
            unknown_publication_dates:r.get(8)?, unknown_workloads:r.get(9)?,
            stale_30_days:r.get(10)?, last_retrieved_at:r.get(11)?,
            last_published_at:r.get(12)?, revision_snapshots:0, exclusion_reasons:Vec::new(),
        })
    }).map_err(|e|e.to_string())?;
    let mut result=Vec::new();
    for row in rows {result.push(row.map_err(|e|e.to_string())?);}
    let mut reason_stmt=db.prepare(r#"
       SELECT legacy_reason,COUNT(*) FROM source_eligibility
       WHERE source=?1 AND classification='excluded' AND TRIM(legacy_reason)<>''
       GROUP BY legacy_reason ORDER BY COUNT(*) DESC,legacy_reason LIMIT 5
    "#).map_err(|e|e.to_string())?;
    let mut revision_stmt=db.prepare(r#"
       SELECT COUNT(*) FROM source_listing_revisions WHERE source=?1
    "#).map_err(|e|e.to_string())?;
    for entry in &mut result {
        entry.revision_snapshots=revision_stmt.query_row([&entry.source],|r|r.get(0)).map_err(|e|e.to_string())?;
        let reasons=reason_stmt.query_map(params![&entry.source], |r| Ok(ExclusionReason{reason:r.get(0)?,count:r.get(1)?})).map_err(|e|e.to_string())?;
        for reason in reasons {entry.exclusion_reasons.push(reason.map_err(|e|e.to_string())?);}
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db()->Connection{
        let db=Connection::open_in_memory().unwrap();
        db.execute_batch(r#"
          CREATE TABLE source_listings(source TEXT,source_id TEXT,description TEXT,published_at TEXT,grades_json TEXT,retrieved_at TEXT);
          CREATE TABLE source_eligibility(source TEXT,source_id TEXT,classification TEXT,legacy_reason TEXT);
          CREATE TABLE source_listing_revisions(source TEXT,source_id TEXT,revision_hash TEXT);
        "#).unwrap();
        db
    }
    #[test]
    fn empty_database_gives_empty_inventory(){assert!(inspect(&db()).unwrap().is_empty());}
    #[test]
    fn counts_descriptions_unknown_dates_and_exclusion_reasons(){
        let db=db();
        db.execute("INSERT INTO source_listings VALUES ('okjob','a','long description','','[]',datetime('now'))",[]).unwrap();
        db.execute("INSERT INTO source_listings VALUES ('okjob','b',NULL,'2026-10-20','[80]',datetime('now','-31 days'))",[]).unwrap();
        db.execute("INSERT INTO source_eligibility VALUES ('okjob','a','excluded','qualification obligatoire')",[]).unwrap();
        db.execute("INSERT INTO source_eligibility VALUES ('okjob','b','unverified','description manquante')",[]).unwrap();
        db.execute("INSERT INTO source_listing_revisions VALUES ('okjob','a','hash1')",[]).unwrap();
        let rows=inspect(&db).unwrap();let x=&rows[0];
        assert_eq!(x.source,"okjob");assert_eq!(x.stored_listings,2);
        assert_eq!(x.excluded,1);assert_eq!(x.unverified,1);
        assert_eq!(x.descriptions_available,1);assert_eq!(x.short_descriptions,1);
        assert_eq!(x.unknown_publication_dates,1);assert_eq!(x.unknown_workloads,1);
        assert_eq!(x.stale_30_days,1);assert_eq!(x.revision_snapshots,1);
        assert_eq!(x.exclusion_reasons[0].count,1);
    }
}
