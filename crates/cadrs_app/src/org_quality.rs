//! The organization screen's Quality module: issues ([`cadrs_org::quality`]).
//!
//! An issue is the one record for anything that went wrong or might: anyone in the organization
//! opens one (here, or from the order it is about), it has an owner, and it is closed with what
//! was decided (rework, repair, use as is, scrap, return to the supplier, no fault) and why.

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, observe};
use cadrs_org::quality::{Disposition, Issue, IssueAbout, IssueStatus};
use cadrs_org::{Op, Org, new_id};
use cadrs_social::keys::AccountId;
use cadrs_ui::dialog::Dialog;
use cadrs_ui::i18n::tr;
use cadrs_ui::input::TextInput;
use cadrs_ui::tab_strip::TabStrip;
use cadrs_ui::theme::Theme;
use cadrs_ui::{Button, Column};

use crate::AppState;
use crate::organization::{
    ModuleView, Names, OpenOrg, OrgDialog, cell_button, choice, close_dialogs, dialog_buttons,
    field, page_head, request_number, status_tag, strip_selected, t_text, table, text_row, write,
};

const STATUSES: [IssueStatus; 3] = [
    IssueStatus::Open,
    IssueStatus::InProgress,
    IssueStatus::Closed,
];

fn status_key(status: IssueStatus) -> &'static str {
    match status {
        IssueStatus::Open => "org-issue-open",
        IssueStatus::InProgress => "org-issue-in-progress",
        IssueStatus::Closed => "org-issue-closed",
    }
}

fn disposition_key(disposition: Disposition) -> &'static str {
    match disposition {
        Disposition::Rework => "org-issue-rework",
        Disposition::Repair => "org-issue-repair",
        Disposition::UseAsIs => "org-issue-use-as-is",
        Disposition::Scrap => "org-issue-scrap",
        Disposition::ReturnToSupplier => "org-issue-return",
        Disposition::NoFault => "org-issue-no-fault",
    }
}

/// An issue's number as people say it: "I-0007".
pub(crate) fn issue_number(number: u32) -> String {
    format!("I-{number:04}")
}

/// What an issue is about, in words.
pub(crate) fn about_label(org: &Org, about: Option<IssueAbout>) -> String {
    let gone = || tr("org-issue-about-gone");
    match about {
        None => "—".into(),
        Some(IssueAbout::Request(id)) => org
            .requests
            .get(&id)
            .map_or_else(gone, |r| request_number(r.number)),
        Some(IssueAbout::Job(id)) => org.jobs.get(&id).map_or_else(gone, |j| {
            let machine = org
                .machines
                .get(&j.machine)
                .map_or_else(String::new, |m| m.name.clone());
            format!("{} · {machine}", tr("org-issue-about-job"))
        }),
        Some(IssueAbout::Part(id)) => org.parts.get(&id).map_or_else(gone, |p| p.name.clone()),
        Some(IssueAbout::Machine(id)) => {
            org.machines.get(&id).map_or_else(gone, |m| m.name.clone())
        }
        Some(IssueAbout::Location(id)) => {
            org.locations.get(&id).map_or_else(gone, |l| l.name.clone())
        }
    }
}

pub(crate) fn issues(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-issues", "org-issues-about");
    let columns = vec![
        Column::new("number", tr("org-issue-number")).width(80.0),
        Column::new("title", tr("org-issue-title")).width(300.0),
        Column::new("about", tr("org-issue-about")).width(150.0),
        Column::new("owner", tr("org-issue-owner")).width(150.0),
        Column::new("status", tr("org-issue-status")).width(110.0),
        Column::new("actions", "").width(90.0),
    ];
    // Open ones first, then the newest.
    let mut listed: Vec<(&cadrs_org::Id, &Issue)> = org.issues.iter().collect();
    listed.sort_by_key(|(_, i)| (i.status == IssueStatus::Closed, std::cmp::Reverse(i.number)));
    let rows = listed
        .into_iter()
        .enumerate()
        .map(|(n, (id, issue))| {
            let id = *id;
            let status = issue.status;
            let tc = t.clone();
            let owner = issue
                .owner
                .map_or_else(|| "—".into(), |o| v.account_name(&o));
            text_row(
                t,
                format!("org-issue-{n}"),
                &columns,
                [
                    issue_number(issue.number),
                    issue.title.clone(),
                    about_label(org, issue.about),
                    owner,
                ],
            )
            .cell(move |c| {
                let color = match status {
                    IssueStatus::Open => tc.link,
                    IssueStatus::InProgress => tc.foreground,
                    IssueStatus::Closed => tc.muted_foreground,
                };
                c.spawn(status_tag(
                    &tc,
                    format!("org-issue-status-{n}"),
                    tr(status_key(status)),
                    color,
                ));
            })
            .cell(move |c| {
                cell_button(
                    c,
                    format!("org-issue-edit-{n}"),
                    tr("common-edit"),
                    false,
                    move |w| open_issue(w, id),
                );
            })
        })
        .collect();
    table(b, t, "org-issues", columns, rows, "org-no-issues");
    b.spawn((
        Button::new("org-add-issue")
            .label(tr("org-issue-new"))
            .icon("plus")
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(|w: &mut World| open_new(w, None));
        }),
    ));
}

/// What the New issue dialog's issue is about.
#[derive(Resource)]
struct NewIssueAbout(Option<IssueAbout>);

/// The issue the dialog moves on.
#[derive(Resource)]
struct EditingIssue(cadrs_org::Id);

/// The members who can own an issue, in the order the dialogs offer them (after "nobody").
fn owners(org: &Org) -> Vec<AccountId> {
    org.members.keys().copied().collect()
}

fn owner_strip(t: &Theme, org: &Org, names: &Names, selected: Option<AccountId>) -> impl Bundle {
    let all = owners(org);
    let at = selected
        .and_then(|s| all.iter().position(|a| *a == s))
        .map_or(0, |i| i + 1);
    let options =
        std::iter::once(tr("org-issue-nobody")).chain(all.iter().map(|account| names.of(account)));
    choice(t, "org-issue-owner", options.collect::<Vec<_>>(), at)
}

fn picked_owner(world: &mut World) -> Option<AccountId> {
    let at = strip_selected(world, "org-issue-owner");
    let org = world.get_resource::<OpenOrg>()?.session.org();
    at.checked_sub(1).and_then(|i| owners(org).get(i).copied())
}

/// Opens the New issue dialog, for an issue about `about` (an order's card passes the order).
pub(crate) fn open_new(world: &mut World, about: Option<IssueAbout>) {
    crate::social::ensure_loaded(world);
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org().clone();
    let names = Names::read(world);
    world.insert_resource(NewIssueAbout(about));
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("org-issue-dialog")
            .title(tr("org-issue-new"))
            .width(600.0)
            .body(move |b| {
                let t = &tb;
                if about.is_some() {
                    b.spawn(t.paragraph(
                        format!("{}: {}", tr("org-issue-about"), about_label(&org, about)),
                        t.font_sm,
                        t.muted_foreground,
                    ));
                }
                b.spawn(t_text(t, "org-issue-title"));
                b.spawn(
                    TextInput::new("org-issue-title")
                        .placeholder(tr("org-issue-title-placeholder"))
                        .autofocus()
                        .build(t),
                );
                b.spawn(t_text(t, "org-issue-details"));
                b.spawn(
                    TextInput::new("org-issue-details")
                        .lines(4)
                        .placeholder(tr("org-issue-details-placeholder"))
                        .build(t),
                );
                b.spawn(t_text(t, "org-issue-owner"));
                // Whoever reports it looks after it until someone else does.
                b.spawn(owner_strip(t, &org, &names, names.me));
            })
            .footer(|f| dialog_buttons(f, "org-issue", add_issue))
            .build(&theme),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
    world.flush();
}

fn add_issue(world: &mut World) {
    let Some(NewIssueAbout(about)) = world.remove_resource::<NewIssueAbout>() else {
        return;
    };
    let title = field(world, "org-issue-title").trim().to_owned();
    if title.is_empty() {
        crate::linked::toast(world, tr("org-issue-needs-title"));
        world.insert_resource(NewIssueAbout(about));
        return;
    }
    let op = Op::OpenIssue {
        id: new_id(),
        title,
        details: field(world, "org-issue-details").trim().to_owned(),
        about,
        owner: picked_owner(world),
    };
    if write(world, op) {
        close_dialogs(world);
        // Opened from an order: show where it went.
        if let Some(mut open) = world.get_resource_mut::<OpenOrg>() {
            open.show(crate::organization::OrgTab::Issues);
        }
    } else {
        world.insert_resource(NewIssueAbout(about));
    }
}

/// Opens an issue: what it says, and its status, owner, decision and reasoning to change.
fn open_issue(world: &mut World, id: cadrs_org::Id) {
    crate::social::ensure_loaded(world);
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org().clone();
    let Some(issue) = org.issues.get(&id).cloned() else {
        return;
    };
    let names = Names::read(world);
    world.insert_resource(EditingIssue(id));
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new("org-issue-edit-dialog")
            .title(format!("{} · {}", issue_number(issue.number), issue.title))
            .width(640.0)
            .body(move |b| {
                let t = &tb;
                let opened_by = names.of(&issue.opened_by);
                b.spawn(t.paragraph(
                    format!(
                        "{}: {} · {}: {opened_by}",
                        tr("org-issue-about"),
                        about_label(&org, issue.about),
                        tr("org-issue-opened-by"),
                    ),
                    t.font_sm,
                    t.muted_foreground,
                ));
                if !issue.details.is_empty() {
                    b.spawn((
                        Name::new("org-issue-details-text"),
                        t.paragraph(issue.details.clone(), t.font_base, t.foreground),
                    ));
                }
                b.spawn(t_text(t, "org-issue-status"));
                let mut status = TabStrip::new("org-issue-status").compact();
                for s in STATUSES {
                    status = status.tab(tr(status_key(s)));
                }
                let at = STATUSES
                    .iter()
                    .position(|s| *s == issue.status)
                    .unwrap_or(0);
                b.spawn(status.selected(at).build(t));
                b.spawn(t_text(t, "org-issue-owner"));
                b.spawn(owner_strip(t, &org, &names, issue.owner));
                b.spawn(t_text(t, "org-issue-disposition"));
                let mut decided = TabStrip::new("org-issue-disposition").compact();
                decided = decided.tab(tr("org-issue-undecided"));
                for d in Disposition::ALL {
                    decided = decided.tab(tr(disposition_key(d)));
                }
                let at = issue
                    .disposition
                    .and_then(|d| Disposition::ALL.iter().position(|x| *x == d))
                    .map_or(0, |i| i + 1);
                b.spawn(decided.selected(at).build(t));
                b.spawn(t_text(t, "org-issue-rationale"));
                b.spawn(
                    TextInput::new("org-issue-rationale")
                        .lines(4)
                        .value(issue.rationale.clone())
                        .placeholder(tr("org-issue-rationale-placeholder"))
                        .build(t),
                );
                b.spawn(t.paragraph(tr("org-issue-close-help"), t.font_sm, t.muted_foreground));
            })
            .footer(|f| dialog_buttons(f, "org-issue-edit", save_issue))
            .build(&theme),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
    world.flush();
}

fn save_issue(world: &mut World) {
    let Some(EditingIssue(id)) = world.remove_resource::<EditingIssue>() else {
        return;
    };
    let status = STATUSES[strip_selected(world, "org-issue-status").min(STATUSES.len() - 1)];
    let disposition = strip_selected(world, "org-issue-disposition")
        .checked_sub(1)
        .and_then(|i| Disposition::ALL.get(i).copied());
    let op = Op::UpdateIssue {
        id,
        status,
        owner: picked_owner(world),
        disposition,
        rationale: field(world, "org-issue-rationale").trim().to_owned(),
    };
    if write(world, op) {
        close_dialogs(world);
    } else {
        world.insert_resource(EditingIssue(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_are_numbered_and_named() {
        assert_eq!(issue_number(7), "I-0007");
        let org = Org::default();
        assert_eq!(about_label(&org, None), "—");
        // What it was about is gone: said so, not left blank.
        assert!(!about_label(&org, Some(IssueAbout::Machine(1))).is_empty());
    }
}
