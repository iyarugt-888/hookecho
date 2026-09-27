use crate::aws::realtime::{ChunkIdentifier, VolumeIndex, REALTIME_BUCKET};
use crate::aws::s3::list_objects;

/// Lists the chunks for the specified radar site and volume. The `max_keys` parameter can be used
/// to limit the number of chunks returned.
///
/// hookecho patch: a volume directory is reused every time the site's 1–999 counter wraps, and
/// the bucket keeps more than one pass through it (e.g. `KVNX/76/` held 67 chunks from three days
/// earlier *and* 67 from today). S3 lists keys in name order, which is oldest pass first, so the
/// first `max_keys` keys were the stale pass: the latest-volume search compared days-old upload
/// times and the live stream joined a volume from the previous day. So the whole directory is
/// listed (a few hundred keys at most, under S3's 1000 per page), only the newest pass — the
/// greatest date-time prefix — is kept, and `max_keys` then applies to that pass.
pub async fn list_chunks_in_volume(
    site: &str,
    volume: VolumeIndex,
    max_keys: usize,
) -> crate::result::Result<Vec<ChunkIdentifier>> {
    let prefix = format!("{}/{}/", site, volume.as_number());
    let list_result = list_objects(REALTIME_BUCKET, &prefix, Some(1000)).await?;

    let metas = list_result
        .objects
        .iter()
        .map(|object| {
            let identifier_segment = object.key.split('/').next_back();
            let identifier = identifier_segment
                .unwrap_or_else(|| object.key.as_ref())
                .to_string();

            ChunkIdentifier::from_name(site.to_string(), volume, identifier, object.last_modified)
        })
        .collect::<crate::result::Result<Vec<_>>>()?;

    Ok(newest_pass(metas, max_keys))
}

/// The chunks of the newest pass through a volume directory (greatest date-time prefix), in
/// sequence order, at most `max_keys` of them.
fn newest_pass(mut metas: Vec<ChunkIdentifier>, max_keys: usize) -> Vec<ChunkIdentifier> {
    let Some(newest) = metas.iter().map(|m| *m.date_time_prefix()).max() else {
        return metas;
    };
    metas.retain(|m| *m.date_time_prefix() == newest);
    metas.sort_by_key(|m| m.sequence());
    metas.truncate(max_keys);
    metas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> ChunkIdentifier {
        ChunkIdentifier::from_name("KVNX".into(), VolumeIndex::new(76), name.into(), None).unwrap()
    }

    #[test]
    fn a_reused_directory_lists_only_its_newest_pass() {
        // Name order, as S3 returns it: the stale pass first.
        let metas = vec![
            id("20260923-130858-001-S"),
            id("20260923-130858-002-I"),
            id("20260923-130858-067-E"),
            id("20260926-171428-001-S"),
            id("20260926-171428-002-I"),
            id("20260926-171428-003-I"),
        ];
        let all = newest_pass(metas.clone(), 100);
        assert_eq!(all.len(), 3);
        assert!(all.iter().all(|m| m.date_time_prefix().to_string().starts_with("2026-09-26")));
        assert_eq!(all.last().unwrap().sequence(), 3, "the latest chunk is last");
        let first = newest_pass(metas, 1);
        assert_eq!(first[0].sequence(), 1, "the newest pass's start chunk");
    }
}
