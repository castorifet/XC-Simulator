
use once_cell::sync::Lazy;

pub const TAG: &str = "qc::x19f";

const K: u8 = 0x4D;

const D: &[&[u8]] = &[
    &[53, 36, 39, 36, 35, 61, 36, 35, 42],
    &[46, 46, 61],
    &[169, 245, 224, 168, 209, 198],
    &[169, 245, 224, 168, 214, 240],
    &[171, 218, 232, 171, 209, 225],
    &[43, 56, 46, 38],
    &[47, 36, 57, 46, 37],
    &[62, 37, 36, 57],
    &[61, 37, 36],
    &[42, 63, 34, 62],
    &[61, 37, 36, 42, 63, 34, 62],
    &[168, 194, 253, 170, 204, 238, 171, 213, 226, 169, 245, 224, 168, 209, 198, 170, 215, 201],
    &[168, 194, 253, 171, 244, 243, 171, 213, 226, 169, 245, 224, 168, 214, 240, 170, 215, 201],
    &[55, 40, 61, 37, 52, 63],
];

static TABLE: Lazy<Vec<String>> = Lazy::new(|| D.iter().filter_map(|e| String::from_utf8(e.iter().map(|b| b ^ K).collect()).ok()).collect());

fn fold(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

fn scan(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let f = fold(s);
    if f.is_empty() {
        return false;
    }

    TABLE.iter().any(|t| t.chars().count() >= 2 && f.contains(t.as_str()))
}

pub fn hit<'a>(parts: impl IntoIterator<Item = &'a str>) -> bool {
    parts.into_iter().any(scan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaced_out_input_is_caught() {

        let spaced: String = TABLE[0].chars().map(|c| format!("{c}-")).collect();
        assert!(hit([format!("name: {spaced} end").as_str()]));
    }

    #[test]
    fn plain_input_is_caught() {
        assert!(hit([format!("xx{}yy", TABLE[9]).as_str()]));
    }

    #[test]
    fn clean_input_passes() {
        assert!(!hit(["Clean", "Composer", "IN Lv.15", "中文歌曲"]));
    }

    #[test]
    fn single_char_overlap_ignored() {

        assert!(!hit(["中", "Lv.9", "Stage 2"]));
    }
}
