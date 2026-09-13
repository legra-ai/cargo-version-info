//! Git commit payload encoding.

/// Build the commit payload that needs to be signed.
///
/// This creates the raw commit object content that git signs. The format is:
/// ```text
/// tree <tree-sha>
/// parent <parent-sha>
/// author <name> <email> <timestamp> <offset>
/// committer <name> <email> <timestamp> <offset>
///
/// <commit message>
/// ```
///
/// Note: The gpgsig header is NOT included in the payload - it's added
/// separately after signing.
pub fn build_commit_payload(
    tree_id: &gix::ObjectId,
    parent_id: gix::Id,
    author: &gix::actor::Signature,
    committer: &gix::actor::Signature,
    message: &str,
) -> Vec<u8> {
    let mut payload = Vec::new();

    // tree <sha>
    payload.extend_from_slice(b"tree ");
    payload.extend_from_slice(tree_id.to_string().as_bytes());
    payload.push(b'\n');

    // parent <sha>
    payload.extend_from_slice(b"parent ");
    payload.extend_from_slice(parent_id.to_string().as_bytes());
    payload.push(b'\n');

    // author <signature>
    payload.extend_from_slice(b"author ");
    write_signature(&mut payload, author);
    payload.push(b'\n');

    // committer <signature>
    payload.extend_from_slice(b"committer ");
    write_signature(&mut payload, committer);
    payload.push(b'\n');

    // blank line before message
    payload.push(b'\n');

    // commit message
    payload.extend_from_slice(message.as_bytes());

    payload
}

/// Write a git signature (author/committer) to the payload buffer.
pub(super) fn write_signature(buf: &mut Vec<u8>, sig: &gix::actor::Signature) {
    // Format: Name <email> timestamp offset
    buf.extend_from_slice(&sig.name);
    buf.extend_from_slice(b" <");
    buf.extend_from_slice(&sig.email);
    buf.extend_from_slice(b"> ");
    buf.extend_from_slice(sig.time.seconds.to_string().as_bytes());
    buf.push(b' ');

    // Format offset as +HHMM or -HHMM
    let offset_minutes = sig.time.offset;
    let sign = if offset_minutes >= 0 { '+' } else { '-' };
    let abs_offset = offset_minutes.abs();
    let hours = abs_offset / 60;
    let minutes = abs_offset % 60;
    buf.extend_from_slice(format!("{}{:02}{:02}", sign, hours, minutes).as_bytes());
}
