mod config;
mod kube;

use std::borrow::Cow;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, ValueEnum};

use crate::config::Config;
use crate::kube::{KubeConfig, SelectOption};

#[derive(Parser, Debug)]
#[command(author, about)]
#[command(disable_help_flag = true)]
#[command(disable_version_flag = true)]
struct Args {
    /// The kubeconfig or namespace name, respect to `-n` flag.
    name: Option<String>,

    /// Edit mode, edit kubeconfig in editor.
    #[clap(long, short)]
    edit: bool,

    /// Delete the kubeconfig file.
    #[clap(long, short)]
    delete: bool,

    /// List kubeconfigs.
    #[clap(long, short)]
    list: bool,

    /// Show current kubeconfig.
    #[clap(long, short)]
    show: bool,

    /// Switch namespace rather than kubeconfig, if enabled, the meaning of NAME changes
    /// to namespace.
    #[clap(long, short)]
    namespace: bool,

    /// Show help about the command.
    #[clap(long, short)]
    help: bool,

    /// Show build info.
    #[clap(long)]
    build: bool,

    /// Show version
    #[clap(long, short)]
    version: bool,

    /// Generate completion items. PLEASE DONOT USE DIRECTLY.
    #[clap(long)]
    comp: bool,

    /// Unset the current kubeconfig.
    #[clap(long, short)]
    unset: bool,

    /// Print the init script, please add `kubeswitch --init <shell-type>` to your
    /// shell profile (etc. ~/.zshrc).
    #[clap(long)]
    init: Option<Shell>,

    /// The wrap target command, change it when your kubeswitch has a different name
    /// or not placed in $PATH.
    #[clap(long, default_value = "kubeswitch")]
    wrap: String,

    /// The completion args. PLEASE DONOT USE DIRECTLY.
    #[clap(last = true)]
    comp_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
}

impl Args {
    fn run(&self, cfg: &Config) -> Result<()> {
        if self.edit {
            return self.run_edit(cfg);
        }
        if self.list {
            return self.run_list(cfg);
        }
        if self.show {
            let kubeconfig = KubeConfig::select(cfg, &None, SelectOption::Current)?;
            eprintln!("{kubeconfig}");
            return Ok(());
        }
        if self.delete {
            return self.run_delete(cfg);
        }
        if self.unset {
            let kubeconfig = KubeConfig::select(cfg, &None, SelectOption::Current)?;
            kubeconfig.unset();
            return Ok(());
        }
        if self.namespace {
            return self.run_namespace(cfg);
        }

        self.run_switch(cfg)
    }

    fn run_edit(&self, cfg: &Config) -> Result<()> {
        let mut kubeconfig = KubeConfig::select(cfg, &self.name, SelectOption::GetOrCreate)?;
        kubeconfig.edit()?;
        kubeconfig.switch()
    }

    fn run_list(&self, cfg: &Config) -> Result<()> {
        let kubeconfigs = KubeConfig::list(cfg)?;
        for kubeconfig in kubeconfigs {
            eprintln!("{kubeconfig}");
        }
        Ok(())
    }

    fn run_delete(&self, cfg: &Config) -> Result<()> {
        let kubeconfig = KubeConfig::select(cfg, &self.name, SelectOption::Get)?;
        kubeconfig.delete()
    }

    fn run_switch(&self, cfg: &Config) -> Result<()> {
        let kubeconfig = KubeConfig::select(cfg, &self.name, SelectOption::Switch)?;
        kubeconfig.switch()
    }

    fn run_namespace(&self, cfg: &Config) -> Result<()> {
        let mut kubeconfig = KubeConfig::select(cfg, &None, SelectOption::Current)?;
        let namespace = kubeconfig.select_namespace(&self.name)?;
        kubeconfig.set_namespace(namespace)?;
        kubeconfig.switch()
    }
}

fn main() -> Result<()> {
    let cfg = Config::load().context("load config")?;

    let args = Args::try_parse()?;
    if args.help {
        let mut cmd = Args::command().name(get_cmd_name(&cfg));
        let help = cmd.render_help();
        eprintln!("{help}");
        return Ok(());
    }

    if args.version {
        show_version(&cfg);
        return Ok(());
    }

    if args.build {
        show_build_info(&cfg);
        return Ok(());
    }

    if args.comp {
        return complete(&cfg, args);
    }

    if let Some(_) = args.init {
        if args.wrap.is_empty() {
            bail!("wrap target cannot be empty");
        }
        show_init(&cfg, args);
        return Ok(());
    }

    args.run(&cfg)
}

fn show_version(cfg: &Config) {
    eprintln!("{} {}", get_cmd_name(&cfg), env!("BUILD_VERSION"));
}

fn show_build_info(cfg: &Config) {
    show_version(cfg);
    eprintln!(
        "rustc {}-{}-{}",
        env!("VERGEN_RUSTC_SEMVER"),
        env!("VERGEN_RUSTC_LLVM_VERSION"),
        env!("VERGEN_RUSTC_CHANNEL")
    );

    eprintln!();
    eprintln!("Build type:   {}", env!("BUILD_TYPE"));
    eprintln!("Build target: {}", env!("BUILD_TARGET"));
    eprintln!("Commit SHA:   {}", env!("BUILD_SHA"));
    eprintln!("Build time:   {}", env!("VERGEN_BUILD_TIMESTAMP"));

    eprintln!();
    let path = match cfg.path.as_ref() {
        Some(path) => Cow::Owned(format!("{}", path.display())),
        None => Cow::Borrowed("N/A"),
    };
    eprintln!("Config path: {path}");
}

fn get_cmd_name(cfg: &Config) -> &'static str {
    Box::leak(cfg.cmd.clone().into_boxed_str())
}

fn show_init(cfg: &Config, args: Args) {
    let wrap = include_bytes!("../scripts/wrap.sh");
    let wrap = String::from_utf8_lossy(wrap).to_string();

    let wrap = wrap.replace("__kubeswitch_cmd", &cfg.cmd);
    let wrap = wrap.replace("__wrap_cmd", &args.wrap);

    println!("{wrap}");
    println!();

    let comp = match args.init.unwrap() {
        Shell::Bash => include_bytes!("../scripts/comp-bash.sh").as_slice(),
        Shell::Zsh => include_bytes!("../scripts/comp-zsh.zsh").as_slice(),
    };
    let comp = String::from_utf8_lossy(comp).to_string();
    let comp = comp.replace("__kubeswitch_cmd", &cfg.cmd);
    let comp = comp.replace("__kubeswitch_comp", &format!("_{}", cfg.cmd));

    println!("{comp}");
}

fn complete(cfg: &Config, args: Args) -> Result<()> {
    let args = args.comp_args.unwrap_or(Vec::new());

    let mut is_namespace = false;
    let mut count = 0;
    let mut to_complete = None;
    for arg in args {
        if !arg.starts_with("-") {
            count += 1;
            to_complete = Some(arg);
            continue;
        }
        let flag = arg.trim_start_matches('-');
        if flag.contains('n') {
            is_namespace = true;
            continue;
        }
        if flag == "namespace" {
            is_namespace = true;
            continue;
        }
    }
    if count > 1 {
        return Ok(());
    }
    let to_complete = to_complete.unwrap_or(String::new());

    let mut items = Vec::new();
    if is_namespace {
        let kubeconfig = KubeConfig::select(cfg, &None, SelectOption::Current)
            .context("select current for completing namespace")?;
        let namespaces = kubeconfig
            .list_namespaces()
            .context("list namespaces for completion")?;

        for ns in namespaces {
            if ns == to_complete {
                return Ok(());
            }
            if ns == kubeconfig.namespace {
                continue;
            }
            if ns.starts_with(&to_complete) {
                items.push(format!("{ns}"));
            }
        }
    } else {
        let kubeconfigs = KubeConfig::list(cfg).context("list kubeconfigs for completion")?;
        for kubeconfig in kubeconfigs {
            if kubeconfig.name == to_complete {
                return Ok(());
            }
            if kubeconfig.current {
                continue;
            }
            if kubeconfig.name.starts_with(&to_complete) {
                items.push(kubeconfig.name);
            }
        }
    }

    for item in items {
        println!("{item}");
    }

    Ok(())
}
