//! The command line. Same engine, same safety: `scan` saves a plan and prints it; `clean`
//! carries out a saved plan only with `--yes` after printing the exact list.

use clap::{Parser, Subcommand};
use cleancraft_core::executor;
use cleancraft_core::features;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::permissions;
use cleancraft_core::plans;
use cleancraft_core::{Env, FeatureId, Settings};

#[derive(Parser)]
#[command(
    name = "cleancraft",
    version,
    about = "Clean and look after your Mac. Nothing is deleted: items move to the Trash and can be put back."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// List features and their ids.
    Features,
    /// Scan a feature (looks only) and save the plan.
    Scan {
        /// Feature id, e.g. system_junk.
        feature: String,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Move the selected items of a saved plan to the Trash (or quarantine).
    Clean {
        plan_id: String,
        /// Confirm the list printed.
        #[arg(long)]
        yes: bool,
    },
    /// Put back one action (id with #) or a whole cleanup (plan id).
    Undo { id: String },
    /// What CleanCraft did.
    History {
        #[arg(long)]
        json: bool,
    },
    /// Full Disk Access status.
    Access,
    /// Live reading: CPU, memory, disk, Trash, battery, network.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Quick Sweep: scan every cleanup module (looks only) and save their plans.
    Sweep,
    /// List Tune-up Tasks with their exact commands.
    Tasks,
    /// Run one Tune-up Task (prints the exact commands; runs only with --yes).
    RunTask {
        id: String,
        /// Confirm the commands printed.
        #[arg(long)]
        yes: bool,
    },
    /// List login items, launch agents and daemons, and heavy apps.
    Startup,
    /// Run the MCP server on stdio (what AI agents launch).
    Serve,
    #[command(flatten)]
    Apps(crate::cli_apps::AppsCmd),
}

pub fn run(args: Vec<String>) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };
    let env = match Env::real() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    match run_cmd(&env, cli.cmd) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e:#}");
            1
        }
    }
}

pub fn run_cmd(env: &Env, cmd: Cmd) -> anyhow::Result<()> {
    let settings = Settings::load(env);
    match cmd {
        Cmd::Features => {
            let scannable: Vec<FeatureId> = features::registry().iter().map(|f| f.id()).collect();
            for f in FeatureId::ALL {
                println!(
                    "{:<18} {}{}",
                    f.as_str(),
                    f.title(),
                    if scannable.contains(&f) {
                        ""
                    } else {
                        "  (no scan)"
                    }
                );
            }
        }
        Cmd::Scan { feature, json } => {
            let f = FeatureId::parse(&feature).ok_or_else(|| {
                anyhow::anyhow!("unknown feature '{feature}' (see `cleancraft features`)")
            })?;
            let plan = features::scan(env, &settings, f)?;
            plans::save(env, &plan)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                for it in &plan.items {
                    let mark = if it.locked.is_some() {
                        "-"
                    } else if it.selected {
                        "x"
                    } else {
                        " "
                    };
                    println!(
                        "[{mark}] {:>10}  {}  ({})",
                        human_size(it.size),
                        env.display(&it.path),
                        it.reason
                    );
                }
                for n in &plan.notes {
                    println!("note: {n}");
                }
                println!("\n{}", plan.summary());
                println!(
                    "Plan {}. To move the selected items to the Trash: cleancraft clean {} --yes",
                    plan.id, plan.id
                );
            }
        }
        Cmd::Clean { plan_id, yes } => {
            let plan = plans::load(env, &plan_id)?;
            for it in plan.selected() {
                println!("{:>10}  {}", human_size(it.size), env.display(&it.path));
            }
            println!("{}", plan.summary());
            if !yes {
                anyhow::bail!("nothing moved: add --yes to confirm this list");
            }
            let rep = features::run_plan(env, &settings, &plan, &plan.digest(), &mut |_, _| {})?;
            println!(
                "{} ({}).",
                cleancraft_core::features::moved_text([&rep]),
                cleancraft_core::fsutil::items(rep.moved.len())
            );
            for (p, why) in &rep.skipped {
                println!("skipped {}: {why}", env.display(p));
            }
            println!("Undo with: cleancraft undo {}", rep.plan_id);
        }
        Cmd::Undo { id } => {
            if id.contains('#') {
                executor::undo(env, &id).map_err(|e| anyhow::anyhow!("{e}"))?;
                println!("Put back.");
            } else {
                let (n, errs) = executor::undo_plan(env, &id);
                println!("{} put back.", cleancraft_core::fsutil::items(n));
                for (id, e) in errs {
                    println!("{id}: {e}");
                }
            }
        }
        Cmd::History { json } => {
            let h = executor::history(env);
            if json {
                println!("{}", serde_json::to_string_pretty(&h)?);
            } else {
                for r in h {
                    println!(
                        "{}  {}  {}{}",
                        r.id,
                        human_size(r.size),
                        serde_json::to_string(&r.kind)?,
                        if r.undone.is_some() { "  (undone)" } else { "" }
                    );
                }
            }
        }
        Cmd::Access => {
            println!("{:?}", permissions::full_disk_access(env));
        }
        Cmd::Status { json } => {
            println!(
                "{}",
                crate::cli_monitor::status(env, json, std::time::Duration::from_secs(1))?
            );
        }
        Cmd::Sweep => println!("{}", crate::cli_monitor::sweep(env, &settings)?),
        Cmd::Apps(c) => crate::cli_apps::run(env, c, &mut std::io::stdout())?,
        Cmd::Tasks => crate::cli_perf::tasks(env, &mut std::io::stdout())?,
        Cmd::RunTask { id, yes } => {
            crate::cli_perf::run_task(env, &id, yes, &mut std::io::stdout())?
        }
        Cmd::Startup => crate::cli_perf::startup(env, &mut std::io::stdout())?,
        Cmd::Serve => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                use rmcp::ServiceExt;
                let service = cleancraft_mcp::CleanCraftMcp::new(env.clone())
                    .serve(rmcp::transport::stdio())
                    .await?;
                service.waiting().await?;
                anyhow::Ok(())
            })?;
        }
    }
    Ok(())
}
