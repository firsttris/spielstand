//! The command line's answers for people, in German; `--json` prints the data instead.

use spielstand_core::custom::{Invalid, MAX_BYTES, MAX_FILES, MAX_PATHS, TooMuch};
use spielstand_protocol::{CustomGame, GameProblem, SaveMode};

use crate::{
    choices,
    client::Refused,
    custom::{Preview, Refusal},
    inspect::{Explained, FoundFile, GameView, Games},
};

fn mode(mode: SaveMode) -> &'static str {
    match mode {
        SaveMode::Sync => "synchronisieren",
        SaveMode::Backup => "nur sichern",
    }
}

pub fn bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

pub fn problem(problem: &GameProblem) -> String {
    match problem {
        GameProblem::NoPrefix => "kein Wine-Prefix für das Spiel gefunden".into(),
        GameProblem::SeveralPrefixes { candidates } => {
            format!(
                "mehrere Prefixe möglich, einer muss gewählt werden: {}",
                candidates.join(", ")
            )
        }
        GameProblem::Failed { message } => format!("Laden fehlgeschlagen: {message}"),
        GameProblem::SeveralEmulators { candidates } => format!(
            "mehrere Emulatoren haben einen Spielstand, einer muss gewählt werden: {}",
            candidates.join(", ")
        ),
        GameProblem::StartInEmulator { emulator } => {
            format!("das Spiel muss einmal in {emulator} gestartet werden")
        }
    }
}

fn count(n: usize) -> String {
    match n {
        1 => "1 Datei".into(),
        n => format!("{n} Dateien"),
    }
}

/// How the saves were found, as the web UI says it.
fn source(source: &str) -> &str {
    match source {
        "custom" => "eigenes Spiel",
        other => other,
    }
}

fn name(view: &GameView) -> String {
    match &view.title {
        Some(title) => format!("{title} ({})", view.report.game),
        None => view.report.game.clone(),
    }
}

/// Why a game is only backed up here although it is set to sync.
fn backup_reason(view: &GameView) -> Option<String> {
    if view.report.effective_mode == view.configured_mode {
        return None;
    }
    if view.report.proposed {
        return Some("eigenes Spiel, wartet auf Übernahme in der Web-UI".into());
    }
    view.report
        .foreign_sync
        .as_ref()
        .map(|f| format!("{} synchronisiert {} auch", f.tool, f.folder))
}

fn game_line(view: &GameView) -> String {
    let mut parts = vec![mode(view.report.effective_mode).to_string()];
    if let Some(found) = &view.report.source {
        parts.push(source(found).to_string());
    }
    if !view.tracked {
        parts.push("noch nicht gesichert".into());
    }
    let mut out = format!("{}  [{}]", name(view), parts.join(", "));
    if let Some(location) = &view.report.location {
        out.push_str(&format!("\n    {location}"));
    }
    if let Some(reason) = backup_reason(view) {
        out.push_str(&format!("\n    nur sichern: {reason}"));
    }
    if let Some(p) = &view.report.problem {
        out.push_str(&format!("\n    unklar: {}", problem(p)));
    }
    out
}

pub fn games(games: &Games) -> String {
    let mut out = format!("{} – {} Spiele", games.device, games.games.len());
    if !games.online {
        out.push_str(" (Server nicht erreichbar)");
    }
    for view in &games.games {
        out.push_str("\n\n");
        out.push_str(&game_line(view));
    }
    out
}

fn files(files: &[FoundFile]) -> String {
    files
        .iter()
        .map(|f| {
            format!(
                "  {}  ({})\n    gespeichert als {}",
                f.path,
                bytes(f.size),
                f.portable
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn explained(e: &Explained) -> String {
    let v = &e.view;
    let mut out = vec![name(v)];
    out.push(format!(
        "Modus: {} (eingestellt: {})",
        mode(v.report.effective_mode),
        mode(v.configured_mode)
    ));
    if let Some(reason) = backup_reason(v) {
        out.push(format!("Nur sichern, weil: {reason}"));
    }
    if let Some(found) = &v.report.source {
        out.push(format!("Gefunden über: {}", source(found)));
    }
    if let Some(prefix) = &v.report.prefix {
        out.push(format!("Prefix: {prefix}"));
    }
    if let Some(p) = &v.report.problem {
        out.push(format!("Unklar: {}", problem(p)));
    }
    out.push(match (&e.head, &e.base) {
        (Some(head), Some(base)) if &head.id == base => "Auf dem neuesten Stand.".into(),
        (Some(_), Some(_)) => "Der Server hat einen neueren Stand.".into(),
        (Some(_), None) => "Der Server hat einen Stand, dieser PC noch keinen.".into(),
        (None, _) => "Der Server hat noch keinen Stand.".into(),
    });
    if e.files.is_empty() {
        out.push("Keine Spielstand-Dateien auf diesem PC.".into());
    } else {
        out.push(format!("{}:\n{}", count(e.files.len()), files(&e.files)));
    }
    out.join("\n")
}

pub fn custom_games(games: &[CustomGame], proposed: &[String]) -> String {
    if games.is_empty() {
        return "Keine eigenen Spiele.".into();
    }
    games
        .iter()
        .map(|g| {
            let scope = match proposed.contains(&g.name) {
                true => "nur dieser PC, wartet auf Übernahme",
                false => "alle PCs",
            };
            let kind = if g.extend {
                ", ergänzt ein bekanntes Spiel"
            } else {
                ""
            };
            format!("{} ({scope}{kind})\n  {}", g.name, g.files.join("\n  "))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn preview(p: &Preview) -> String {
    format!(
        "{}: {}, {}\n{}",
        p.game.name,
        count(p.files.len()),
        bytes(p.total_bytes),
        files(&p.files)
    )
}

fn invalid(e: &Invalid) -> String {
    match e {
        Invalid::NoName => "Der Name fehlt.".into(),
        Invalid::SwitchName => "Namen der Form „Switch <Title-ID>“ gehören den Switch-Spielen.".into(),
        Invalid::NoPaths => "Es fehlt ein Pfad (--path).".into(),
        Invalid::TooManyPaths => format!("Höchstens {MAX_PATHS} Pfade."),
        Invalid::NoAnchor(p) => {
            format!("{p}: Ein Pfad beginnt mit einem Platzhalter wie <home>, <xdgData> oder <winAppData>.")
        }
        Invalid::TooBroad(p) => {
            format!("{p}: Nach dem Platzhalter muss ein Ordner mit Namen stehen, kein Muster.")
        }
        Invalid::Parent(p) => format!("{p}: „..“ ist nicht erlaubt."),
        Invalid::Protected(p) => {
            format!("{p}: Dieser Ordner enthält Schlüssel oder Passwörter, keine Spielstände.")
        }
        Invalid::InstallDir(d) => format!("{d}: Ein Installationsordner ist ein einzelner Ordnername."),
    }
}

pub fn refusal(r: &Refusal) -> String {
    match r {
        Refusal::Invalid(e) => invalid(e),
        Refusal::InManifest => {
            "Das Manifest kennt dieses Spiel schon. Liegen die Spielstände hier woanders, \
             ergänze es mit --extend."
                .into()
        }
        Refusal::Empty => "Die Pfade passen auf keine Datei auf diesem PC. Gibt es noch keinen Spielstand, \
             erlaubt --allow-empty das Eintragen trotzdem."
            .into(),
        Refusal::TooMuch(TooMuch::Files(n)) => {
            format!("{n} Dateien sind zu viele (höchstens {MAX_FILES}); der Pfad fasst mehr als Spielstände.")
        }
        Refusal::TooMuch(TooMuch::Bytes(n)) => format!(
            "{} sind zu viel (höchstens {}); der Pfad fasst mehr als Spielstände.",
            bytes(*n),
            bytes(MAX_BYTES)
        ),
        Refusal::Overlap { file, game } => format!("{file} gehört schon zu {game}."),
    }
}

pub fn choice_refusal(r: &choices::Refusal) -> String {
    match r {
        choices::Refusal::Missing(p) => format!("{} gibt es nicht.", p.display()),
        choices::Refusal::NotAPrefix(p) => format!("{} ist kein Wine-Prefix.", p.display()),
        choices::Refusal::NotACandidate(c) if c.is_empty() => {
            "Auf diesem PC hat kein Emulator einen Spielstand dieses Spiels.".into()
        }
        choices::Refusal::NotACandidate(c) => format!("Zur Wahl stehen: {}", c.join(", ")),
    }
}

/// A refusal from the server, in the words of the command line.
pub fn refused(r: &Refused) -> String {
    match r.status {
        409 if r.message.contains("every device") => {
            "Dieses eigene Spiel gilt für alle PCs; ändern oder entfernen lässt es sich in der Web-UI.".into()
        }
        409 => "Ein anderer PC hat schon ein eigenes Spiel dieses Namens vorgeschlagen.".into(),
        404 => "Das gibt es auf dem Server nicht.".into(),
        401 => "Der Server kennt diesen PC nicht mehr; koppele ihn neu (spielstand-agent pair).".into(),
        _ => format!("Der Server lehnt ab: {}", r.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_people_write_them() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(3 << 20), "3.0 MB");
    }

    #[test]
    fn a_refusal_from_the_server_is_explained() {
        let r = Refused {
            status: 409,
            message: "this custom game is in effect on every device; change it in the web UI".into(),
        };
        assert!(refused(&r).contains("Web-UI"));
    }
}
