mod blocks;
mod code;
mod commands;
#[cfg(test)]
#[cfg_attr(
    test,
    allow(
        dead_code,
        reason = "Legacy cloud scaffolding remains compiled for tests but is not registered in Warp Local"
    )
)]
mod conversations;
mod diffset;
mod files;
pub mod mixer;
mod notebooks;
mod rules;
pub mod search;
#[cfg(all(not(target_family = "wasm"), test))]
#[cfg_attr(
    test,
    allow(
        dead_code,
        reason = "Legacy cloud scaffolding remains compiled for tests but is not registered in Warp Local"
    )
)]
mod skills;
mod styles;
pub mod view;
mod workflows;

/// Safely truncate a string at the given byte index, ensuring we don't split UTF-8 characters
pub fn safe_truncate(s: &mut String, new_len: usize) {
    if new_len >= s.len() {
        return;
    }
    let safe_len = floor_char_boundary(s, new_len);
    s.truncate(safe_len);
}

/// Find the largest valid character boundary at or before the given byte index
pub fn floor_char_boundary(original_string: &str, idx: usize) -> usize {
    if idx >= original_string.len() {
        original_string.len()
    } else {
        let mut curr = idx;
        while curr > 0 && !original_string.is_char_boundary(curr) {
            curr -= 1;
        }
        curr
    }
}
