use super::*;

#[test]
fn test_exact_mismatch_repro() {
    let a = [
        23, 24, 19, 9, 1, 25, 26, 27, 26, 28, 26, 29, 30, 28, 30, 31, 1, 20, 32, 16, 33,
    ];
    let b = [
        111, 3, 112, 9, 254, 1, 255, 30, 238, 1, 20, 30, 23, 85, 256, 180, 1, 46, 26, 29, 26, 1,
        194, 16,
    ];
    let b_token_chars = [
        4, 1, 3, 1, 4, 1, 6, 1, 2, 1, 1, 1, 3, 7, 5, 2, 1, 5, 1, 6, 1, 1, 2, 3,
    ];
    let ws = vec![false; 300];
    let mut alnum = vec![false; 300];
    let a_alnum = [
        true, true, true, false, false, true, false, true, false, true, false, true, false, true,
        false, true, false, false, true, true, true,
    ];
    let b_alnum = [
        true, false, true, false, true, false, true, false, true, false, false, false, true, true,
        true, true, false, true, false, true, false, false, true, true,
    ];
    for (&id, &is_al) in a.iter().zip(a_alnum.iter()) {
        alnum[id as usize] = is_al;
    }
    for (&id, &is_al) in b.iter().zip(b_alnum.iter()) {
        alnum[id as usize] = is_al;
    }

    let bp_suffix = bit_parallel_suffix_matched_chars_64(&a, &b, &ws, &alnum, &b_token_chars);
    assert_eq!(bp_suffix, 15);
}

#[test]
fn test_differential_against_2d_lcs() {
    let ws = vec![false; 256];
    let alnum = vec![true; 256];
    let mut scratch = Vec::new();

    // Test varying sequence lengths from 1 to 60
    for len in [1, 5, 12, 25, 40, 60] {
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut b_chars = Vec::new();

        for i in 0..len {
            let token_a = ((i * 7 + 3) % 23) as u32;
            let token_b = ((i * 11 + 5) % 23) as u32;
            a.push(token_a);
            b.push(token_b);
            b_chars.push((token_b as usize % 10) + 1);
        }

        let bp_matched = bit_parallel_matched_chars_64(&a, &b, &ws, &b_chars);
        let scalar_matched =
            crate::lcs::lcs_matched_chars_ids(&a, &b, &ws, &alnum, &b_chars, &mut scratch);

        assert_eq!(
            bp_matched, scalar_matched,
            "Mismatch for len {len}: bp={bp_matched}, scalar={scalar_matched}"
        );
    }
}

#[test]
fn test_differential_128_against_2d_lcs() {
    let ws = vec![false; 256];
    let alnum = vec![true; 256];
    let mut scratch = Vec::new();

    for len in [65, 80, 100, 128] {
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut b_chars = Vec::new();

        for i in 0..len {
            let token_a = ((i * 13 + 7) % 37) as u32;
            let token_b = ((i * 17 + 11) % 37) as u32;
            a.push(token_a);
            b.push(token_b);
            b_chars.push((token_b as usize % 10) + 1);
        }

        let bp_matched = bit_parallel_matched_chars_128(&a, &b, &ws, &b_chars);
        let scalar_matched =
            crate::lcs::lcs_matched_chars_ids(&a, &b, &ws, &alnum, &b_chars, &mut scratch);

        assert_eq!(
            bp_matched, scalar_matched,
            "Mismatch for len {len}: bp={bp_matched}, scalar={scalar_matched}"
        );
    }
}

#[test]
fn test_fuzz_differential_with_duplicates_and_lengths() {
    let ws = vec![false; 128];
    let alnum = vec![true; 128];
    let mut scratch = Vec::new();

    // Pseudo-random deterministic generator
    let mut seed = 0xdead_beef_cafe_babe_u64;
    let mut rng = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    let mut token_lens = vec![0usize; 128];
    for len in &mut token_lens {
        *len = ((rng() % 8) + 1) as usize;
    }

    for _ in 0..10_000 {
        let len_a = ((rng() % 35) + 1) as usize; // up to 35 tokens
        let len_b = ((rng() % 50) + 1) as usize; // up to 50 tokens
        let mut a = Vec::with_capacity(len_a);
        let mut b = Vec::with_capacity(len_b);
        let mut b_chars = Vec::with_capacity(len_b);

        for _ in 0..len_a {
            // small alphabet 0..15 creates lots of duplicates and ties
            a.push((rng() % 15) as u32);
        }
        for _ in 0..len_b {
            let id = (rng() % 15) as u32;
            b.push(id);
            b_chars.push(token_lens[id as usize]);
        }

        let bp = bit_parallel_matched_chars_64(&a, &b, &ws, &b_chars);
        let sc = crate::lcs::lcs_matched_chars_ids(&a, &b, &ws, &alnum, &b_chars, &mut scratch);

        // Print detail on difference if any
        if bp != sc {
            eprintln!("Difference detected! bp={bp}, sc={sc}");
            eprintln!("a = {a:?}");
            eprintln!("b = {b:?}");
            eprintln!("b_chars = {b_chars:?}");
        }
        assert_eq!(bp, sc, "Mismatch between bit-parallel and scalar LCS");
    }
}

fn scalar_suffix_matched_chars_ref(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
) -> usize {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return 0;
    }
    let stride = m + 1;
    let mut dp = vec![0u32; (n + 1) * stride];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * stride + j] = if a[i] == b[j] {
                dp[(i + 1) * stride + (j + 1)] + 1
            } else {
                dp[(i + 1) * stride + j].max(dp[i * stride + (j + 1)])
            };
        }
    }
    if dp[0] == 0 {
        return 0;
    }
    let mut matched = 0usize;
    let mut i = 0;
    let mut j = 0;
    let mut in_add_run = false;
    while i < n && j < m {
        if a[i] == b[j] {
            if id_ws[a[i] as usize]
                && in_add_run
                && !(i + 1 < n && j + 1 < m && a[i + 1] == b[j + 1] && !id_ws[a[i + 1] as usize])
            {
                j += 1;
                continue;
            }
            if in_add_run
                && dp[i * stride + j + 1] == dp[i * stride + j]
                && (j + 1..m)
                    .any(|k| b[k] == a[i] && dp[(i + 1) * stride + k + 1] + 1 == dp[i * stride + j])
            {
                j += 1;
                continue;
            }
            matched += b_token_chars[j];
            i += 1;
            j += 1;
            in_add_run = false;
        } else if dp[(i + 1) * stride + j] > dp[i * stride + j + 1] {
            i += 1;
        } else if dp[(i + 1) * stride + j] < dp[i * stride + j + 1] {
            j += 1;
            in_add_run = true;
        } else if id_ws[a[i] as usize] && !id_ws[b[j] as usize] {
            if i + 1 < n && a[i + 1] == b[j] {
                i += 1;
            } else {
                j += 1;
                in_add_run = true;
            }
        } else if id_ws[b[j] as usize] && !id_ws[a[i] as usize] {
            j += 1;
            in_add_run = true;
        } else if !b[j + 1..].contains(&a[i])
            || (!id_alnum[a[i] as usize] && id_alnum[b[j] as usize])
        {
            i += 1;
        } else {
            j += 1;
            in_add_run = true;
        }
    }
    matched
}

#[test]
fn test_fuzz_suffix_differential_with_ws_and_alnum() {
    let mut seed = 0xcafe_babe_dead_beef_u64;
    let mut rng = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    let mut ws = vec![false; 32];
    let mut alnum = vec![false; 32];
    let mut token_lens = vec![0usize; 32];

    // Tokens 0..3: whitespace
    ws[0..4].fill(true);

    // Tokens 4..19: alphanumeric
    alnum[4..20].fill(true);

    // Tokens 20..31: delimiters (remain ws=false, alnum=false by default)

    // Assign character lengths to all 32 synthetic token IDs (1 for
    // whitespace, 1..7 for others)
    for i in 0..32 {
        token_lens[i] = if ws[i] { 1 } else { (i % 7) + 1 };
    }

    // Test 64-bit word
    for _ in 0..5_000 {
        let len_a = ((rng() % 35) + 1) as usize;
        let len_b = ((rng() % 50) + 1) as usize;
        let mut a = Vec::with_capacity(len_a);
        let mut b = Vec::with_capacity(len_b);
        let mut b_chars = Vec::with_capacity(len_b);

        for _ in 0..len_a {
            a.push((rng() % 24) as u32);
        }
        for _ in 0..len_b {
            let id = (rng() % 24) as u32;
            b.push(id);
            b_chars.push(token_lens[id as usize]);
        }

        let bp = bit_parallel_suffix_matched_chars_64(&a, &b, &ws, &alnum, &b_chars);
        let sc = scalar_suffix_matched_chars_ref(&a, &b, &ws, &alnum, &b_chars);

        assert_eq!(
            bp, sc,
            "Suffix 64 mismatch! bp={bp}, sc={sc}\na={a:?}\nb={b:?}"
        );
    }

    // Test 128-bit word
    for _ in 0..2_000 {
        let len_a = ((rng() % 45) + 65) as usize; // 65..110 tokens
        let len_b = ((rng() % 60) + 40) as usize; // 40..100 tokens
        let mut a = Vec::with_capacity(len_a);
        let mut b = Vec::with_capacity(len_b);
        let mut b_chars = Vec::with_capacity(len_b);

        for _ in 0..len_a {
            a.push((rng() % 28) as u32);
        }
        for _ in 0..len_b {
            let id = (rng() % 28) as u32;
            b.push(id);
            b_chars.push(token_lens[id as usize]);
        }

        let bp = bit_parallel_suffix_matched_chars_128(&a, &b, &ws, &alnum, &b_chars);
        let sc = scalar_suffix_matched_chars_ref(&a, &b, &ws, &alnum, &b_chars);

        assert_eq!(
            bp, sc,
            "Suffix 128 mismatch! bp={bp}, sc={sc}\na={a:?}\nb={b:?}"
        );
    }
}

#[test]
fn test_suffix_matched_chars_above_128() {
    let mut ws = vec![false; 64];
    let mut alnum = vec![false; 64];
    let mut token_lens = vec![1usize; 64];
    ws[0..4].fill(true);
    alnum[4..30].fill(true);
    for i in 0..64 {
        token_lens[i] = if ws[i] { 1 } else { (i % 7) + 1 };
    }

    let mut seed = 0x1234_5678_9abc_def0_u64;
    let mut rng = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    for len_a in [150, 250, 350, 600, 1200] {
        let len_b = len_a + 50;
        let mut a = Vec::with_capacity(len_a);
        let mut b = Vec::with_capacity(len_b);
        let mut b_chars = Vec::with_capacity(len_b);

        for _ in 0..len_a {
            a.push((rng() % 40) as u32);
        }
        for _ in 0..len_b {
            let id = (rng() % 40) as u32;
            b.push(id);
            b_chars.push(token_lens[id as usize]);
        }

        let bp = bit_parallel_suffix_matched_chars(&a, &b, &ws, &alnum, &b_chars);
        assert!(bp.is_some(), "Expected Some for len_a = {len_a}");
        let sc = scalar_suffix_matched_chars_ref(&a, &b, &ws, &alnum, &b_chars);
        assert_eq!(bp.unwrap(), sc, "Mismatch for len_a = {len_a}");
    }
}

#[test]
fn test_suffix_matched_chars_scratch_reuse() {
    let ws = vec![false; 64];
    let alnum = vec![true; 64];
    let a = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let b = vec![2, 4, 6, 8, 10];
    let b_chars = vec![1; 5];
    let mut scratch = Vec::new();
    let res1 =
        bit_parallel_suffix_matched_chars_scratch(&a, &b, &ws, &alnum, &b_chars, &mut scratch);
    let cap_before = scratch.capacity();
    assert!(cap_before > 0);
    let res2 =
        bit_parallel_suffix_matched_chars_scratch(&a, &b, &ws, &alnum, &b_chars, &mut scratch);
    assert_eq!(res1, res2);
    assert_eq!(
        scratch.capacity(),
        cap_before,
        "scratch capacity should be reused without reallocating"
    );
}

#[test]
fn test_suffix_pattern_matches_direct() {
    let ws = vec![false; 64];
    let alnum = vec![true; 64];
    let mut scratch1 = Vec::new();
    let mut scratch2 = Vec::new();

    for len_a in [0, 10, 50, 100, 200, 400, 800, 1200, 2200] {
        let len_b = len_a + 20;
        let a: Vec<u32> = (0..len_a).map(|x| (x % 30) as u32).collect();
        let b1: Vec<u32> = (0..len_b).map(|x| ((x * 2) % 30) as u32).collect();
        let b2: Vec<u32> = (0..len_b).map(|x| ((x * 3) % 30) as u32).collect();
        let b1_chars = vec![1; b1.len()];
        let b2_chars = vec![1; b2.len()];

        let pat = SuffixPattern::new(&a);
        let m1_pat = pat.match_chars(&b1, &ws, &alnum, &b1_chars, &mut scratch1);
        let m1_dir = bit_parallel_suffix_matched_chars_scratch(
            &a,
            &b1,
            &ws,
            &alnum,
            &b1_chars,
            &mut scratch2,
        );
        assert_eq!(m1_pat, m1_dir, "mismatch on b1 for len_a = {len_a}");

        let m2_pat = pat.match_chars(&b2, &ws, &alnum, &b2_chars, &mut scratch1);
        let m2_dir = bit_parallel_suffix_matched_chars_scratch(
            &a,
            &b2,
            &ws,
            &alnum,
            &b2_chars,
            &mut scratch2,
        );
        assert_eq!(m2_pat, m2_dir, "mismatch on b2 for len_a = {len_a}");
    }
}
