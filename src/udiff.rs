use difflib::{sequencematcher::Opcode, sequencematcher::SequenceMatcher};
use std::{
  collections::HashSet,
  fmt::{self, Display, Formatter},
};

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct DiffRange {
  pub before: (usize, usize),
  pub after: (usize, usize),
}

impl DiffRange {
  pub fn new(ops: &[Opcode]) -> Option<DiffRange> {
    match (ops.first(), ops.last()) {
      (Some(first), Some(last)) => Some(DiffRange {
        before: (first.first_start, last.first_end - first.first_start),
        after: (first.second_start, last.second_end - first.second_start),
      }),
      _ => None,
    }
  }
}

impl Display for DiffRange {
  fn fmt(&self, f: &mut Formatter) -> fmt::Result {
    write!(
      f,
      "@@ -{},{} +{},{} @@",
      self.before.0 + 1,
      self.before.1,
      self.after.0 + 1,
      self.after.1,
    )
  }
}

pub type DiffRanges = Vec<DiffRange>;

pub trait Picker {
  fn new(unified: usize, before: &str, after: &str) -> Self;
}

impl Picker for DiffRanges {
  fn new(unified: usize, before: &str, after: &str) -> Self {
    let before = before.lines().collect::<Vec<&str>>();
    let after = after.lines().collect::<Vec<&str>>();
    let mut ret = Vec::new();
    let mut matcher = SequenceMatcher::new(&before, &after);
    for group in &matcher.get_grouped_opcodes(unified) {
      let range = DiffRange::new(group).unwrap();
      ret.push(range);
    }
    ret
  }
}

pub struct Diff {
  range: DiffRange,
  new_lines: Vec<String>,
}

pub type Diffs = Vec<Diff>;

pub trait Patchable {
  fn new(unified: usize, before: &str, after: &str) -> Self;
  fn patch(&self, ranges: &HashSet<DiffRange>, before: &str) -> String;
}

impl Patchable for Diffs {
  fn new(unified: usize, before: &str, after: &str) -> Self {
    let before = before.lines().collect::<Vec<&str>>();
    let after = after.lines().collect::<Vec<&str>>();

    let mut ret = Vec::new();
    let mut matcher = SequenceMatcher::new(&before, &after);

    for group in &matcher.get_grouped_opcodes(unified) {
      let mut new_lines = Vec::new();
      for code in group {
        if code.tag == "equal" {
          for line in before.iter().take(code.first_end).skip(code.first_start) {
            new_lines.push((*line).to_string());
          }
          continue;
        }
        if code.tag == "replace" || code.tag == "insert" {
          for line in after.iter().take(code.second_end).skip(code.second_start) {
            new_lines.push((*line).to_string());
          }
        }
      }
      let diff = Diff {
        range: DiffRange::new(group).unwrap(),
        new_lines,
      };
      ret.push(diff);
    }
    ret
  }

  fn patch(&self, ranges: &HashSet<DiffRange>, before: &str) -> String {
    let before = before.lines().collect::<Vec<&str>>();
    let mut ret = String::new();
    let mut prev = 0;

    for diff in self.iter() {
      let (before_start, before_inc) = diff.range.before;
      let before_end = before_start + before_inc;
      for i in prev..before_start {
        before.get(i).map(|b| ret.push_str(b)).unwrap();
        ret.push('\n');
      }
      if ranges.contains(&diff.range) {
        for line in diff.new_lines.iter() {
          ret.push_str(line);
          ret.push('\n')
        }
      } else {
        for i in before_start..before_end {
          before.get(i).map(|b| ret.push_str(b)).unwrap();
          ret.push('\n')
        }
      }
      prev = before_end;
    }
    for i in prev..before.len() {
      before.get(i).map(|b| ret.push_str(b)).unwrap();
      ret.push('\n')
    }
    ret
  }
}

pub fn udiff(
  ranges: Option<&HashSet<DiffRange>>,
  unified: usize,
  name: &str,
  before: &str,
  after: &str,
) -> String {
  let before = before.lines().collect::<Vec<&str>>();
  let after = after.lines().collect::<Vec<&str>>();

  let mut ret = String::new();
  ret.push_str(&format!("\ndiff --git {} {}", name, name));
  ret.push_str(&format!("\n--- {}", name));
  ret.push_str(&format!("\n+++ {}", name));

  let mut matcher = SequenceMatcher::new(&before, &after);
  for group in &matcher.get_grouped_opcodes(unified) {
    let range = DiffRange::new(group).unwrap();
    if let Some(ranges) = &ranges {
      if !ranges.contains(&range) {
        continue;
      }
    };
    ret.push_str(&format!("\n{}", range));
    for code in group {
      if code.tag == "equal" {
        for line in before.iter().take(code.first_end).skip(code.first_start) {
          ret.push_str(&format!("\n {}", *line))
        }
        continue;
      }
      if code.tag == "replace" || code.tag == "delete" {
        for line in before.iter().take(code.first_end).skip(code.first_start) {
          ret.push_str(&format!("\n-{}", *line))
        }
      }
      if code.tag == "replace" || code.tag == "insert" {
        for line in after.iter().take(code.second_end).skip(code.second_start) {
          ret.push_str(&format!("\n+{}", *line))
        }
      }
    }
  }
  ret
}

#[cfg(test)]
mod tests {
  use super::*;
  use difflib::unified_diff;
  use regex::Regex;
  use std::{fs, path::PathBuf, collections::HashSet};

  fn read_files() -> Vec<String> {
    let path = PathBuf::from("src");
    fs::read_dir(path)
      .unwrap()
      .map(|entry| {
        let path = entry.unwrap().path();
        fs::read_to_string(path).unwrap()
      })
      .collect::<Vec<String>>()
  }

  fn regexes() -> Vec<(Regex, String)> {
    vec![("std", "owo")]
      .into_iter()
      .map(|(s1, s2)| (Regex::new(s1).unwrap(), s2.to_string()))
      .collect::<Vec<(Regex, String)>>()
  }

  fn diffs() -> Vec<(String, String)> {
    let texts = read_files();
    let regexes = regexes();
    let mut acc = Vec::new();
    for text in texts {
      for re in &regexes {
        let before = text.clone();
        let after = re.0.replace_all(text.as_str(), re.1.as_str());
        acc.push((before, after.to_string()))
      }
    }
    acc
  }

  #[test]
  fn patch() {
    let diffs = diffs();
    for (before, after) in diffs {
        let unified = 3;
        let ranges: DiffRanges = Picker::new(unified, &before, &after);
        let rangeset = ranges.into_iter().collect::<HashSet<DiffRange>>();
        let diffs: Diffs = Patchable::new(unified, &before, &after);
        let patched = diffs.patch(&rangeset, &before);
        assert_eq!(after, patched);
    }
  }

  #[test]
  fn unified() {
    let diffs = diffs();
    for (before, after) in diffs {
        let unified = 3;
        let bb = before.lines().collect::<Vec<&str>>();
        let aa = after.lines().collect::<Vec<&str>>();
        let canon = unified_diff(&bb, &aa, "", "", "", "", unified).join("\n");
        let imp = udiff(None, unified, "", &before, &after);
        assert_eq!(canon, imp);
    }
  }
}
