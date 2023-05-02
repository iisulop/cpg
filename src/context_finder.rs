use std::ops::Range;

use regex::Regex;

use crate::error::CpgError;

pub enum InputType {
    Git,
}

pub struct ContextFinder {
    start: Regex,
    end: Regex,
}

impl ContextFinder {
    pub fn new(input_type: InputType) -> Result<Self, CpgError> {
        match input_type {
            InputType::Git => {
                let start = Regex::new(r"^commit [0-9a-fA-F]{40}").unwrap();
                let end = Regex::new(r"^(commit [0-9a-fA-F]{40}|diff --git)").unwrap();
                Ok(ContextFinder { start, end })
            }
        }
    }

    pub fn get_context<'a>(
        &self,
        all_lines: &'a [String],
        position: usize,
    ) -> Option<&'a [String]> {
        let context_lines = self.find_range(all_lines, position);
        if let Some(lines) = context_lines {
            all_lines.get(lines.start..(lines.end + 1))
        } else {
            None
        }
    }

    fn find_range(&self, lines: &[String], current_position: usize) -> Option<Range<usize>> {
        if let Some(context_start_position) = self.start_line_num(lines, current_position) {
            if let Some(context_end_delta) =
                self.end_line_num(lines, current_position, context_start_position)
            {
                Some(Range {
                    start: context_start_position,
                    end: context_start_position + context_end_delta,
                })
            } else {
                Some(Range {
                    start: context_start_position,
                    end: current_position - 1,
                })
            }
        } else {
            None
        }
    }

    fn start_line_num(&self, lines: &[String], start_position: usize) -> Option<usize> {
        let pos = lines.get(0..start_position).map(|lines| {
            lines
                .iter()
                .enumerate()
                .rev()
                .find(|(_line_num, line)| self.start.is_match(line))
        });
        pos.unwrap_or(None).map(|(num, _line)| num)
    }

    fn end_line_num(
        &self,
        lines: &[String],
        start_position: usize,
        start_line_num: usize,
    ) -> Option<usize> {
        let pos = lines
            .get((start_line_num + 1)..start_position)
            .map(|lines| {
                lines
                    .iter()
                    .enumerate()
                    .find(|(_line_num, line)| self.end.is_match(line))
            });
        pos.unwrap_or(None).map(|(num, _line)| num)
    }
}

#[cfg(test)]
mod test {
    use std::io::BufRead;

    use crate::{context_finder::ContextFinder, error::CpgError};

    pub const GIT_LOG: &str = include_str!("../tests/data/git_patch");

    fn read_input<R: BufRead>(mut reader: R) -> Result<String, CpgError> {
        let mut buf: Vec<u8> = Vec::new();
        reader.read_to_end(&mut buf)?;
        let result = String::from_utf8_lossy(&buf);
        Ok(result.to_string())
    }

    #[test]
    fn read_file() {
        let input = GIT_LOG.repeat(10);
        let buf = read_input(input.as_bytes()).unwrap();
        assert_eq!(input, buf);
    }

    #[test]
    fn find_commit_from_start() {
        let lines = GIT_LOG.lines();
        let input: Vec<String> = lines.map(|l| l.to_string()).collect();
        let cf = ContextFinder::new(crate::context_finder::InputType::Git).unwrap();
        let commit_pos = cf.find_range(&input, 0);
        assert!(commit_pos.is_none());
    }

    #[test]
    fn find_commit_from_end() {
        let lines = GIT_LOG.lines();
        let input: Vec<String> = lines.map(|l| l.to_string()).collect();
        let cf = ContextFinder::new(crate::context_finder::InputType::Git).unwrap();
        let range = cf.find_range(&input, input.len() - 1).unwrap();
        assert_eq!(range.start, 306);
        assert_eq!(range.end, 311);
        assert!(input[range.start].contains("commit"));
        assert!(input[range.start + 1].contains("Mr. Example"));
    }

    #[test]
    fn find_commit_patch_from_start() {
        let lines = GIT_LOG.lines();
        let input: Vec<String> = lines.map(|l| l.to_string()).collect();
        let cf = ContextFinder::new(crate::context_finder::InputType::Git).unwrap();
        let range = cf.find_range(&input, 0);
        assert!(range.is_none());
    }

    #[test]
    fn find_commit_patch_first() {
        let lines = GIT_LOG.lines();
        let input: Vec<String> = lines.map(|l| l.to_string()).collect();
        let cf = ContextFinder::new(crate::context_finder::InputType::Git).unwrap();
        let range = cf.find_range(&input, 10).unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 5);
        assert!(input[range.start].contains("commit"));
        assert!(input[range.start + 1].contains("Mr. Example"));
    }

    #[test]
    fn find_commit_patch() {
        let lines = GIT_LOG.lines();
        let input: Vec<String> = lines.map(|l| l.to_string()).collect();
        let cf = ContextFinder::new(crate::context_finder::InputType::Git).unwrap();
        let range = cf.find_range(&input, input.len() - 1).unwrap();
        assert_eq!(range.start, 306);
        assert_eq!(range.end, 311);
        assert!(input[range.start].contains("commit"));
        assert!(input[range.start + 1].contains("Mr. Example"));
    }
}
