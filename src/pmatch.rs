/*
 * Copyright (c) 2019 Jonathan Perkin <jonathan@perkin.org.uk>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 *
 * pmatch.rs - implement pkg_match() for package pattern matching
 */

/*!
 * Implements pkg_match()
 *
 * ## Examples
 *
 * ```
 * use pkgsrc::pmatch::pkg_match;
 *
 * // simple match
 * assert_eq!(pkg_match("foobar-1.0", "foobar-1.0"), true);
 * assert_eq!(pkg_match("foobar-1.0", "foobar-1.1"), false);
 *
 * // dewey comparisons
 * assert_eq!(pkg_match("foobar>=1.0", "foobar-1.1"), true);
 * assert_eq!(pkg_match("foobar>=1.1", "foobar-1.0"), false);
 *
 * // alternate matches
 * assert_eq!(pkg_match("{foo,bar}>=1.0", "foo-1.1"), true);
 * assert_eq!(pkg_match("{foo,bar}>=1.0", "bar-1.1"), true);
 * assert_eq!(pkg_match("{foo,bar}>=1.0", "moo-1.1"), false);
 *
 * // globs
 * assert_eq!(pkg_match("foo-[0-9]*", "foo-1.0"), true);
 * assert_eq!(pkg_match("fo?-[0-9]*", "foo-1.0"), true);
 * assert_eq!(pkg_match("fo*-[0-9]*", "foobar-1.0"), true);
 * ```
 */
use glob;

fn alternate_match(pattern: &str, pkg: &str) -> bool {
    let mut found = false;
    let v_open: Vec<_> = pattern.match_indices('{').collect();
    let v_close: Vec<_> = pattern.match_indices('}').collect();
    if v_open.len() != v_close.len() || v_open.is_empty() {
        eprintln!("ERROR: Malformed alternate match '{}'", pattern);
        return false;
    }

    for (i, _) in v_open.iter().rev() {
        let (first, rest) = pattern.split_at(*i);
        let n = rest.find('}').unwrap();
        let (matches, last) = rest.split_at(n + 1);
        let matches = &matches[1..matches.len() - 1];

        for m in matches.split(',') {
            let fmt = format!("{}{}{}", first, m, last);
            if pkg_match(&fmt, pkg) {
                found = true;
            }
        }
    }

    found
}

/*
 * pkg_install implements "==" (DEWEY_EQ) and "!=" (DEWEY_NE) but doesn't
 * actually support them (or document them), so we don't bother.
 */
#[derive(Debug, PartialEq)]
enum DeweyOp {
    LE,
    LT,
    GE,
    GT,
}

fn dewey_get_op(pattern: &str) -> (DeweyOp, usize) {
    if pattern.starts_with(">=") {
        (DeweyOp::GE, 2)
    } else if pattern.starts_with('>') {
        (DeweyOp::GT, 1)
    } else if pattern.starts_with("<=") {
        (DeweyOp::LE, 2)
    } else if pattern.starts_with('<') {
        (DeweyOp::LT, 1)
    } else {
        panic!("Bad DeweyOp pattern, this can't happen?");
    }
}

fn dewey_mkvec(pattern: &str) -> (Vec<i64>, i64) {
    let mut vec: Vec<i64> = Vec::new();
    let mut idx = 0;
    let mut nb: i64 = 0;

    if !pattern.is_ascii() {
        eprintln!("WARNING: Invalid non-ASCII pattern: {}", pattern);
        return (vec, nb);
    }

    loop {
        if idx == pattern.len() {
            break;
        }

        let pat_slice = &pattern[idx..pattern.len()];

        if pat_slice.starts_with("alpha") {
            vec.push(-3);
            idx += 5;
        } else if pat_slice.starts_with("beta") {
            vec.push(-2);
            idx += 4;
        } else if pat_slice.starts_with("rc") {
            vec.push(-1);
            idx += 2;
        } else if pat_slice.starts_with("pl") {
            vec.push(0);
            idx += 2;
        } else if pat_slice.starts_with('.') || pat_slice.starts_with('_') {
            vec.push(0);
            idx += 1;
        } else if pat_slice.starts_with("nb") {
            idx += 2;
            for c in pattern[idx..pattern.len()].chars() {
                let num = c.to_digit(10);
                if num.is_none() {
                    break;
                }
                nb = i64::from((nb * 10) as u32 + num.unwrap());
                idx += 1;
            }
            if nb == 0 {
                eprintln!("WARNING: Bad dewey version: {}", pattern);
            }
        } else if pat_slice.chars().next().unwrap().is_ascii_digit() {
            let nums = pat_slice.chars().take_while(|d| d.is_ascii_digit());
            let mut n: i64 = 0;
            for num in nums {
                n = i64::from(num.to_digit(10).unwrap());
                idx += 1;
            }
            vec.push(n);
        } else if pat_slice.chars().next().unwrap().is_ascii_alphabetic() {
            vec.push(0);
            vec.push(pat_slice.chars().next().unwrap() as i64);
            idx += 1;
        } else {
            eprintln!(
                "WARNING: Invalid char '{}' in dewey pattern '{}'",
                pat_slice.chars().next().unwrap(),
                pattern
            );
            idx += 1;
        }
    }

    (vec, nb)
}

fn dewey_test(lhs: i64, op: &DeweyOp, rhs: i64) -> bool {
    match op {
        DeweyOp::GE => lhs >= rhs,
        DeweyOp::GT => lhs > rhs,
        DeweyOp::LE => lhs <= rhs,
        DeweyOp::LT => lhs < rhs,
    }
}

/*
 * Compare two
 */
fn dewey_cmp(lhs: &str, op: &DeweyOp, rhs: &str) -> bool {
    let (mut lhs_vec, lhs_nb) = dewey_mkvec(lhs);
    let (mut rhs_vec, rhs_nb) = dewey_mkvec(rhs);

    /*
     * Make both vectors the same size, filling space with 0.
     */
    if lhs_vec.len() < rhs_vec.len() {
        lhs_vec.resize(rhs_vec.len(), 0);
    } else if rhs_vec.len() < lhs_vec.len() {
        rhs_vec.resize(lhs_vec.len(), 0);
    }

    /*
     * If any items are different then we can exit early.
     */
    for (i, _item) in lhs_vec.iter().enumerate() {
        if lhs_vec[i] == rhs_vec[i] {
            continue;
        }
        return dewey_test(lhs_vec[i], op, rhs_vec[i]);
    }

    /*
     * If we weren't able to exit early then leave it to the nb<x> comparison.
     * This ensures, even if nb is unused, that e.g. "foo>1" "foo-1.0" is
     * correctly handled as that will successfully pass the above.
     */
    dewey_test(lhs_nb, op, rhs_nb)
}

/*
 * Dewey matches compare the version to ensure it is within the specified
 * bounds.  Only plain package names are matched.
 */
fn dewey_match(pattern: &str, pkg: &str) -> bool {
    /* Extract package name and version comparison from pattern */
    let mut pattern_idx = match pattern.find(|c: char| c == '<' || c == '>') {
        Some(i) => i,
        None => return false,
    };
    let (pattern_pkgname, pattern_op) = pattern.split_at(pattern_idx);

    /* Extract package name and version from pkg */
    let v: Vec<&str> = pkg.rsplitn(2, '-').collect();
    if v.len() != 2 {
        return false;
    }
    /* These are in reverse order from rsplitn() */
    let pkg_pkgname = v[1];
    let pkg_version = v[0];

    /*
     * Ensure that the package name is identical.  Only exact matches are
     * supported, no globs etc.
     */
    if pattern_pkgname != pkg_pkgname {
        return false;
    }

    /*
     * Extract comparison operator(s)
     */
    let (op, incr) = dewey_get_op(pattern_op);
    pattern_idx += incr;
    let (_, mut pattern_version) = pattern.split_at(pattern_idx);

    /* If > or >= look for a second < or <= operator for limited matches */
    if op == DeweyOp::GT || op == DeweyOp::GE {
        if let Some(_bad) = pattern_version.find('>') {
            eprintln!("WARNING: Invalid dewey pattern: {}", pattern);
            return false;
        }
        if let Some(n) = pattern_version.find('<') {
            let (newpv, sep2) = pattern_version.split_at(n);
            let (op2, incr2) = dewey_get_op(sep2);
            let (_, pattern_version2) = pattern_version.split_at(n + incr2);
            pattern_version = newpv;
            if let Some(_bad) = pattern_version2.find('<') {
                eprintln!("WARNING: Invalid dewey pattern: {}", pattern);
                return false;
            }
            if !dewey_cmp(&pkg_version, &op2, &pattern_version2) {
                return false;
            }
        }
    }
    if !dewey_cmp(&pkg_version, &op, &pattern_version) {
        return false;
    }

    true
}

/*
 * For glob matching just use the external glob crate.
 */
fn glob_match(pattern: &str, pkg: &str) -> bool {
    glob::Pattern::new(pattern).unwrap().matches(pkg)
}

/*
 * pkg_install contains a quick_pkg_match() routine to quickly exit if
 * there is no possibility of a match.  As it gives a decent speed bump we
 * include a similar routine.
 */
fn is_simple_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

fn quick_pkg_match(pattern: &str, pkg: &str) -> bool {
    let mut p1 = pattern.chars();
    let mut p2 = pkg.chars();
    let mut p;

    p = p1.next();
    if p.is_none() || !is_simple_char(p.unwrap()) {
        return true;
    }
    if p != p2.next() {
        return false;
    }

    p = p1.next();
    if p.is_none() || !is_simple_char(p.unwrap()) {
        return true;
    }
    if p != p2.next() {
        return false;
    }
    true
}

/**
 * Compare package `pkg` against pattern `pattern`.
 */
pub fn pkg_match(pattern: &str, pkg: &str) -> bool {
    /* Bail out early if the simple match test fails */
    if !quick_pkg_match(pattern, pkg) {
        return false;
    }

    /*
     * csh-style {foo,bar} alternates
     */
    if pattern.contains('{') {
        return alternate_match(pattern, pkg);
    }

    /*
     * dewey match
     */
    if pattern.contains('>') || pattern.contains('<') {
        return dewey_match(pattern, pkg);
    }

    /*
     * glob match
     */
    if (pattern.contains('*')
        || pattern.contains('?')
        || pattern.contains('[')
        || pattern.contains(']'))
        && glob_match(pattern, pkg)
    {
        return true;
    }

    /*
     * Simple match
     */
    if pattern == pkg {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /*
     * csh-style alternate matches, i.e. {this,that}.
     */
    fn pkg_match_alternate() {
        /* Valid matches */
        assert_eq!(pkg_match("a-{b,c}-{d{e,f},g}-h>=1", "a-b-de-h-2.0"), true);
        assert_eq!(pkg_match("a-{b,c}-{d{e,f},g}-h>=1", "a-c-df-h-2.0"), true);
        assert_eq!(pkg_match("a-{b,c}-{d{e,f},g}-h>=1", "a-c-g-h-2.0"), true);
        /* Glob matches cannot be used with dewey. */
        assert_eq!(pkg_match("{foo,b?r}*-[0-9]*", "baring-1.0"), true);
        /* Bad syntax */
        assert_eq!(pkg_match("{foo,bar}}>=1", "foo-1.0"), false);
        assert_eq!(pkg_match("{{foo,bar}>=1", "foo-1.0"), false);
    }

    /*
     * Dewey matches, identical package name and valid version constraint.
     */
    #[test]
    fn pkg_match_dewey() {
        /* Valid version matches */
        assert_eq!(pkg_match("foo>1", "foo-1.1"), true);
        assert_eq!(pkg_match("foo>1", "foo-1.0pl1"), true);
        assert_eq!(pkg_match("foo<1", "foo-1.0alpha1"), true);
        assert_eq!(pkg_match("foo>=1", "foo-1.0"), true);
        assert_eq!(pkg_match("foo<2", "foo-1.0"), true);
        assert_eq!(pkg_match("foo>=1", "foo-1.0"), true);
        assert_eq!(pkg_match("foo>=1<2", "foo-1.0"), true);
        assert_eq!(pkg_match("foo>1<2", "foo-1.0nb2"), true);
        /* Valid version non-matches */
        assert_eq!(pkg_match("foo>1<2", "foo-2.5"), false);
        assert_eq!(pkg_match("foo>1", "foo-0.5"), false);
        assert_eq!(pkg_match("foo>1", "foo-1.0"), false);
        assert_eq!(pkg_match("foo>1", "foo-1.0alpha1"), false);
        assert_eq!(pkg_match("foo>1nb3", "foo-1.0nb2"), false);
        assert_eq!(pkg_match("foo>1<2", "foo-0.5"), false);
        assert_eq!(pkg_match("bar>=1", "foo-1.0"), false);
        /*
         * This looks like a bad package name but we accept it, pkg_install
         * simply performs comparisons on any trailing characters.
         */
        assert_eq!(pkg_match("foo>1.1", "foo-1.1blah2"), true);
        assert_eq!(pkg_match("foo>1.1a2", "foo-1.1blah2"), true);
        assert_eq!(pkg_match("foo>1.1c2", "foo-1.1blah2"), false);
        /*
         * Bad patterns
         */
        assert_eq!(pkg_match("foo>=1", "foo"), false);
        assert_eq!(pkg_match("foo>", "foo"), false);
        assert_eq!(pkg_match("foo>=1<2<3", "foo-1.0"), false);
        assert_eq!(pkg_match("foo>=1<2>3", "foo-1.0"), false);
    }

    #[test]
    fn pkg_match_glob() {
        assert_eq!(pkg_match("foo-[0-9]*", "foo-1.0"), true);
        assert_eq!(pkg_match("fo?-[0-9]*", "foo-1.0"), true);
        assert_eq!(pkg_match("fo*-[0-9]*", "foo-1.0"), true);
        assert_eq!(pkg_match("?oo-[0-9]*", "foo-1.0"), true);
        assert_eq!(pkg_match("*oo-[0-9]*", "foo-1.0"), true);
        assert_eq!(pkg_match("foo-[0-9]", "foo-1"), true);

        assert_eq!(pkg_match("boo-[0-9]*", "foo-1.0"), false);
        assert_eq!(pkg_match("bo?-[0-9]*", "foo-1.0"), false);
        assert_eq!(pkg_match("bo*-[0-9]*", "foo-1.0"), false);

        assert_eq!(pkg_match("foo-[2-9]*", "foo-1.0"), false);
        assert_eq!(pkg_match("fo-[0-9]*", "foo-1.0"), false);
        assert_eq!(pkg_match("bar-[0-9]*", "foo-1.0"), false);
    }

    /*
     * Simple package matches, no version comparison or glob.
     */
    #[test]
    fn pkg_match_simple() {
        assert_eq!(pkg_match("foo-1.0", "foo-1.0"), true);
        assert_eq!(pkg_match("foo-1.1", "foo-1.0"), false);
        assert_eq!(pkg_match("bar-1.0", "foo-1.0"), false);
    }
}
