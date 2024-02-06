mod config;
mod context;

use std::borrow::Cow;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, ValueEnum};
use regex::Regex;

use crate::config::Config;
use crate::context::{KubeContext, SelectOption};

#[derive(Parser, Debug)]
#[command(author, about)]
#[command(disable_help_flag = true)]
#[command(disable_version_flag = true)]
struct Args {
    /// The context or namespace name, respect to `-n` flag.
    name: Option<String>,

    /// Edit mode, edit context's kubeconfig file in editor.
    #[clap(long, short)]
    edit: bool,

    /// Delete the context, its kubeconfig file will be deleted.
    #[clap(long, short)]
    delete: bool,

    /// List contexts.
    #[clap(long, short)]
    list: bool,

    /// Show current context.
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

    /// Create a symbol link context, the format is "{source}:{dest}".
    #[clap(long)]
    link: bool,

    /// Show version
    #[clap(long, short)]
    version: bool,

    /// Generate completion items. PLEASE DONOT USE DIRECTLY.
    #[clap(long)]
    comp: bool,

    /// Unset the current context.
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
            let ctx = KubeContext::current(cfg)?;
            eprintln!("{ctx}");
            return Ok(());
        }
        if self.delete {
            return self.run_delete(cfg);
        }
        if self.unset {
            let ctx = KubeContext::current(cfg)?;
            ctx.unset();
            return Ok(());
        }
        if self.link {
            return self.run_link(cfg);
        }
        if self.namespace {
            return self.run_namespace(cfg);
        }

        self.run_switch(cfg)
    }

    fn run_edit(&self, cfg: &Config) -> Result<()> {
        let mut ctx = KubeContext::select(cfg, &self.name, SelectOption::GetNotRequired)?;
        ctx.edit()?;
        ctx.switch()
    }

    fn run_list(&self, cfg: &Config) -> Result<()> {
        let ctxs = KubeContext::list(cfg)?;
        for ctx in ctxs {
            if ctx.current {
                println!("* {ctx}");
                continue;
            }
            println!("{ctx}");
        }
        Ok(())
    }

    fn run_delete(&self, cfg: &Config) -> Result<()> {
        let ctx = KubeContext::select(cfg, &self.name, SelectOption::GetRequired)?;
        ctx.delete()
    }

    fn run_switch(&self, cfg: &Config) -> Result<()> {
        let ctx = KubeContext::select(cfg, &self.name, SelectOption::Switch)?;
        ctx.switch()
    }

    fn run_namespace(&self, cfg: &Config) -> Result<()> {
        let mut ctx = KubeContext::current(cfg)?;
        let namespace = ctx.select_namespace(&self.name)?;
        ctx.set_namespace(namespace)?;
        ctx.switch()
    }

    fn run_link(&self, cfg: &Config) -> Result<()> {
        use crate::context::create_symlink;

        if self.name.is_none() {
            bail!("missing link target");
        }

        create_symlink(cfg, self.name.as_ref().unwrap())
    }
}

const NAME_REGEX: &'static str = "^[a-zA-Z-_0-9/:]+$";

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

    if let Some(name) = args.name.as_ref() {
        if name.is_empty() {
            bail!("invalid input name, should not be empty");
        }
        let re = Regex::new(NAME_REGEX).unwrap();
        if !re.is_match(name) {
            bail!("invalid input name, should not contain special character");
        }

        if name.contains(":") && !args.link {
            bail!("invalid input name, should not contain ':'");
        }
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
        let ctx =
            KubeContext::current(cfg).context("get current context for completing namespace")?;
        let namespaces = ctx
            .list_namespaces()
            .context("list namespaces for completion")?;

        for ns in namespaces {
            if ns == to_complete {
                return Ok(());
            }
            if ns == ctx.namespace {
                continue;
            }
            if ns.starts_with(&to_complete) {
                items.push(format!("{ns}"));
            }
        }
    } else {
        let ctxs = KubeContext::list(cfg).context("list contexts for completion")?;
        for ctx in ctxs {
            if ctx.name == to_complete {
                return Ok(());
            }
            if ctx.current {
                continue;
            }
            if ctx.name.starts_with(&to_complete) {
                items.push(ctx.name);
            }
        }
    }

    for item in items {
        println!("{item}");
    }

    Ok(())
}
