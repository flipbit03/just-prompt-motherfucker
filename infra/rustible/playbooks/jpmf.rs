//! jpmf: the machine-shaping this site needs.
//!
//! Only what a deploy cannot renew on its own. The binary, the systemd unit
//! and the .env are written by .github/workflows/deploy.yml on every release;
//! jpmf.db is written by nothing but the app itself.
//!
//! Precondition: /etc/caddy/Caddyfile imports /etc/caddy/conf.d/*.caddy, and
//! that directory exists. The Caddyfile belongs to the machine's own
//! provisioning, so this playbook writes one file into conf.d/ and nothing else.

use rustible::prelude::*;
use rustible_github as github;
use rustible_std::{file, shell, ssh, systemd, user};

/// The deploy workflow logs in with this key; the private half is the
/// repository's DEPLOY_SSH_KEY secret.
const DEPLOY_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHoXKDhF2HHwF7wK5q/loDi68zAO1gGV9EkaTyli5noN jpmf-deploy";

/// Apex redirects to www, www proxies to the binary. `encode` because the
/// page is entirely text.
const VHOST: &str = "\
just-prompt-motherfucker.com {
        redir https://www.just-prompt-motherfucker.com{uri} permanent
}

www.just-prompt-motherfucker.com {
        encode zstd gzip
        reverse_proxy 127.0.0.1:8100
}
";

#[rustible::playbook(hosts = "outpost", escalate = true)]
fn main(ctx: &mut Ctx) -> Result<()> {
    let jpmf = ctx.step(
        "jpmf user",
        user::Present::new("jpmf")
            .shell("/bin/bash")
            .create_home(true),
    )?;

    // Check mode withholds the output of a step that would change, and every
    // step below reads this account. On a machine where the user does not
    // exist yet there is nothing further to check, so say so and stop rather
    // than panicking on the first field access.
    if !jpmf.is_available() {
        ctx.log("jpmf does not exist yet; the steps after it cannot be checked until it does");
        return Ok(());
    }

    // ssh::authorized_keys refuses to create this and says so; it has to
    // exist before the next step.
    ctx.step(
        "~jpmf/.ssh",
        file::Directory::at(jpmf.home.join(".ssh"))
            .owner(jpmf.uid, jpmf.gid)
            .mode(0o700),
    )?;

    ctx.step(
        "deploy key",
        ssh::authorized_keys::Present::for_user(&jpmf).keys([DEPLOY_KEY]),
    )?;

    // Additive, so it can never remove the deploy key above.
    github::github_ssh_keys_to_user(ctx, "flipbit03", jpmf.name.as_str())?;

    // Without lingering, the user manager stops at logout and the service
    // does not come back after a reboot. enable-linger creates this file,
    // which is therefore also the idempotence check.
    ctx.step(
        "linger",
        shell::Command::new("loginctl")
            .args(["enable-linger", jpmf.name.as_str()])
            .creates(format!("/var/lib/systemd/linger/{}", jpmf.name)),
    )?;

    let vhost = ctx.step(
        "caddy vhost",
        file::Copy::from_str(VHOST)
            .to("/etc/caddy/conf.d/jpmf.caddy")
            .mode(0o644),
    )?;

    // Reload only on a change, and reload rather than restart: Caddy keeps
    // serving the old config if the new one fails to load.
    if vhost.changed {
        ctx.step("reload caddy", systemd::Reload::new("caddy"))?;
    }

    Ok(())
}
