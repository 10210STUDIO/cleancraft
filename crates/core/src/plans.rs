//! Saved plans, so the CLI and agents can scan in one step and confirm in another:
//! `<data>/plans/<plan id>.json`. A saved plan is only a preview; executing it still goes
//! through the executor's fence and the digest check.

use std::io;
use std::path::PathBuf;

use crate::env::Env;
use crate::plan::Plan;

pub fn dir(env: &Env) -> PathBuf {
    env.data_dir().join("plans")
}

pub fn save(env: &Env, plan: &Plan) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir(env))?;
    let p = dir(env).join(format!("{}.json", plan.id));
    std::fs::write(
        &p,
        serde_json::to_vec_pretty(plan).map_err(io::Error::other)?,
    )?;
    Ok(p)
}

pub fn load(env: &Env, id: &str) -> io::Result<Plan> {
    if id.contains('/') || id.contains("..") {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad plan id"));
    }
    let b = std::fs::read(dir(env).join(format!("{id}.json")))?;
    serde_json::from_slice(&b).map_err(io::Error::other)
}
