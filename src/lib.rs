use std::io::{self, BufRead};

use regex::Regex;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CpgError {
    #[error("Could not initialize terminal")]
    IoErr(#[from] io::Error),
}

pub fn parse_git_lines(lines: &[&str], pos: usize) -> Result<Option<(usize, usize)>, CpgError> {
    let commit_start_regex = r"^commit [0-9a-fA-F]{40}";
    let commit_end_regex = r"^(commit [0-9a-fA-F]{40}|diff --git)";

    let start_regex = Regex::new(commit_start_regex).unwrap();
    let end_regex = Regex::new(commit_end_regex).unwrap();

    if let Some(Some((start_line_num, _start_line))) = lines.get(0..pos).map(|lines| {
        lines
            .iter()
            .enumerate()
            .rev()
            .find(|(_line_num, line)| start_regex.is_match(line))
    }) {
        if let Some(Some((end_line_num, _end_line))) =
            lines.get((start_line_num + 1)..pos).map(|lines| {
                lines
                    .iter()
                    .enumerate()
                    .find(|(_line_num, line)| end_regex.is_match(line))
            })
        {
            Ok(Some((start_line_num, start_line_num + end_line_num)))
        } else {
            // Some(start line num) , None
            Ok(Some((start_line_num, pos - 1)))
        }
    } else {
        Ok(None)
    }
}

pub fn read_input<R: BufRead>(mut reader: R) -> Result<String, CpgError> {
    let mut buf: Vec<u8> = Vec::new();
    reader.read_to_end(&mut buf)?;
    let result = String::from_utf8_lossy(&buf);
    Ok(result.to_string())
    // Ok(reader.read_to_string(buf)?)
}

#[cfg(test)]
mod test {
    use crate::{parse_git_lines, read_input};

    pub const GIT_LOG: &str = include_str!("../tests/data/git_patch");

    #[test]
    fn read_file() {
        let input = GIT_LOG.repeat(10);
        let buf = read_input(input.as_bytes()).unwrap();
        assert_eq!(input, buf);
    }

    #[test]
    fn find_commit_from_start() {
        let lines = GIT_LOG.lines();
        let input: Vec<&str> = lines.collect();
        let commit_pos = parse_git_lines(&input, 0).unwrap();
        assert!(commit_pos.is_none());
    }

    #[test]
    fn find_commit_from_end() {
        let lines = GIT_LOG.lines();
        let input: Vec<&str> = lines.collect();
        let (start, end) = parse_git_lines(&input, input.len() - 1).unwrap().unwrap();
        dbg!(start);
        dbg!(end);
    }

    #[test]
    fn find_commit_patch_from_start() {
        let lines = GIT_LOG.lines();
        let input: Vec<&str> = lines.collect();
        let commit_pos = parse_git_lines(&input, 0).unwrap();
        assert!(commit_pos.is_none());
    }

    #[test]
    fn find_commit_patch_first() {
        let lines = GIT_LOG.lines();
        let input: Vec<&str> = lines.collect();
        let (start, end) = parse_git_lines(&input, 10).unwrap().unwrap();
        dbg!(start);
        dbg!(end);
        println!("{:#?}", &input[start..end]);
    }

    #[test]
    fn find_commit_patch() {
        let lines = GIT_LOG.lines();
        let input: Vec<&str> = lines.collect();
        let (start, end) = parse_git_lines(&input, input.len() - 1).unwrap().unwrap();
        dbg!(start);
        dbg!(end);
    }
}
