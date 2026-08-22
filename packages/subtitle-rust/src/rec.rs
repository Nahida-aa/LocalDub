//! CTC decode for recognition output.
//!
//! Expects logits of shape `[timesteps, num_classes]` (best-effort: we
//! accept either this or `[1, timesteps, num_classes]` and flatten the
//! leading dims). Collapses repeated labels, drops blanks, takes the
//! per-char max probability to produce a confidence score averaged over the
//! non-blank characters.

pub fn ctc_decode(logits: &[f32], shape: &[usize], char_list: &[String]) -> (String, f32) {
    // Locate the last two dims = (timesteps, num_classes).
    if shape.len() < 2 {
        return (String::new(), 0.0);
    }
    let num_classes = shape[shape.len() - 1];
    let timesteps = shape[shape.len() - 2];
    // Guard against overlong dims: check logits length.
    if timesteps * num_classes > logits.len() {
        return (String::new(), 0.0);
    }

    let mut chars = String::new();
    let mut confs = Vec::<f32>::new();
    let mut prev: i32 = -1; // -1 = blank equivalent (merge with label 0)
    for t in 0..timesteps {
        let row = &logits[t * num_classes..(t + 1) * num_classes];
        let mut max_idx = 0usize;
        let mut max_val = row[0];
        for (i, &v) in row.iter().enumerate() {
            if v > max_val {
                max_val = v;
                max_idx = i;
            }
        }
        if max_idx == 0 {
            // blank
            prev = -1;
            continue;
        }
        if (max_idx as i32) != prev {
            if max_idx < char_list.len() {
                chars.push_str(&char_list[max_idx]);
                confs.push(max_val);
            }
        }
        prev = max_idx as i32;
    }
    let avg = if confs.is_empty() {
        0.0
    } else {
        confs.iter().sum::<f32>() / confs.len() as f32
    };
    (chars, avg)
}
