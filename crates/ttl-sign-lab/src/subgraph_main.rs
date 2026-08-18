//! Reduce a sanitized VM trace to the reachable subgraph of each confirmed signing route.
//!
//! This reads an artifact; it never drives a browser and never signs. The output is deterministic
//! and safe to commit.

use std::path::PathBuf;

use anyhow::{Context, Result};
use ttl_sign_lab::{
    default_routes, extract_subgraphs, read_vm_trace, subgraph_document_json,
    ControlledObservation, RouteName, RouteSpec,
};

fn main() -> Result<()> {
    let arguments = arguments()?;
    let report = read_vm_trace(&arguments.trace)?;

    let controlled: Vec<ControlledObservation> = match &arguments.controlled {
        Some(path) => {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            serde_json::from_str(&raw).context("invalid controlled-observation file")?
        }
        None => Vec::new(),
    };

    let routes = if arguments.routes.is_empty() {
        default_routes()
    } else {
        let selected: Vec<RouteSpec> = default_routes()
            .into_iter()
            .filter(|spec| arguments.routes.contains(&spec.route))
            .collect();
        anyhow::ensure!(!selected.is_empty(), "no known route selected");
        selected
    };

    let document = extract_subgraphs(&report, &routes, &controlled)?;
    let json = subgraph_document_json(&document);
    match arguments.output {
        Some(path) => {
            std::fs::write(&path, json)
                .with_context(|| format!("cannot write {}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }
        None => print!("{json}"),
    }
    Ok(())
}

struct Arguments {
    trace: PathBuf,
    output: Option<PathBuf>,
    controlled: Option<PathBuf>,
    routes: Vec<RouteName>,
}

fn arguments() -> Result<Arguments> {
    let usage = "usage: ttl-sign-subgraph <vm-trace.json> [--output <path>] \
                 [--controlled <observations.json>] [--route <name>]...\n\
                 routes: fetch_composition | ms_token | x_dynosaur | x_gnarly | frontier_x_bogus";
    let mut args = std::env::args().skip(1);
    let trace = PathBuf::from(args.next().context(usage)?);
    let mut parsed = Arguments {
        trace,
        output: None,
        controlled: None,
        routes: Vec::new(),
    };
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--output" => parsed.output = Some(PathBuf::from(args.next().context(usage)?)),
            "--controlled" => parsed.controlled = Some(PathBuf::from(args.next().context(usage)?)),
            "--route" => {
                let name = args.next().context(usage)?;
                let route: RouteName =
                    serde_json::from_value(serde_json::Value::String(name)).context(usage)?;
                parsed.routes.push(route);
            }
            _ => anyhow::bail!(usage),
        }
    }
    Ok(parsed)
}
