//! Compiled operation route matching and path-parameter capture.

use std::collections::{BTreeMap, BTreeSet};

use axum::http::Method;
use simile_artifact::Artifact;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RouteTable {
    routes: Vec<Route>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Route {
    method: String,
    path: PathTemplate,
    operation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PathTemplate {
    segments: Vec<PathSegment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PathSegment {
    Literal(String),
    Parameter(String),
}

pub(super) enum Match<'a> {
    Operation {
        operation: &'a str,
        path: BTreeMap<String, Vec<String>>,
    },
    MethodNotAllowed(BTreeSet<&'a str>),
    NotFound,
}

impl RouteTable {
    pub(super) fn new(artifact: &Artifact) -> Self {
        let mut routes = artifact
            .operations()
            .map(|operation| Route {
                method: operation.method().to_owned(),
                path: PathTemplate::parse(operation.path()),
                operation: operation.key().to_owned(),
            })
            .collect::<Vec<_>>();
        routes.sort_by(|left, right| {
            right
                .path
                .literal_count()
                .cmp(&left.path.literal_count())
                .then_with(|| left.operation.cmp(&right.operation))
        });
        Self { routes }
    }

    pub(super) fn find(&self, method: &Method, path: &str) -> Match<'_> {
        let mut allowed = BTreeSet::new();
        for route in &self.routes {
            let Some(parameters) = route.path.match_and_capture(path) else {
                continue;
            };
            if route.method == method.as_str() {
                return Match::Operation {
                    operation: &route.operation,
                    path: parameters,
                };
            }
            allowed.insert(route.method.as_str());
        }
        if allowed.is_empty() {
            Match::NotFound
        } else {
            Match::MethodNotAllowed(allowed)
        }
    }
}

impl PathTemplate {
    fn parse(path: &str) -> Self {
        let segments = split_path(path)
            .map(|segment| {
                if segment.starts_with('{') && segment.ends_with('}') {
                    PathSegment::Parameter(
                        segment
                            .trim_start_matches('{')
                            .trim_end_matches('}')
                            .to_owned(),
                    )
                } else {
                    PathSegment::Literal(segment.to_owned())
                }
            })
            .collect();
        Self { segments }
    }

    fn match_and_capture(&self, path: &str) -> Option<BTreeMap<String, Vec<String>>> {
        let actual = split_path(path).collect::<Vec<_>>();
        if self.segments.len() != actual.len() {
            return None;
        }
        let mut captured = BTreeMap::new();
        for (expected, actual) in self.segments.iter().zip(actual) {
            match expected {
                PathSegment::Literal(expected) if expected == actual => {}
                PathSegment::Literal(_) => return None,
                PathSegment::Parameter(name) if !actual.is_empty() => {
                    let decoded = percent_encoding::percent_decode_str(actual)
                        .decode_utf8()
                        .ok()?
                        .into_owned();
                    captured.insert(name.clone(), vec![decoded]);
                }
                PathSegment::Parameter(_) => return None,
            }
        }
        Some(captured)
    }

    fn literal_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| matches!(segment, PathSegment::Literal(_)))
            .count()
    }
}

fn split_path(path: &str) -> impl Iterator<Item = &str> {
    path.trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
}
