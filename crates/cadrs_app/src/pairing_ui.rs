//! Linking another device (a phone, a second computer) to this account, and joining one
//! (the account menu's *Link a device…* and *Join my account…*; `cadrs_social::pairing`).
//!
//! 1. *Link a device…* shows a QR code (and its text): a one-time offer naming this account and
//!    this device. cadrs goes online to hear the answer.
//! 2. On the other device, *Join my account…* takes the code (scanned or pasted), and sends the
//!    request here. Both screens then show the same six digits.
//! 3. The owner checks they match and confirms here: this device signs the other's certificate
//!    and sends it back. From then on the other device speaks for the account.

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_social::keys::{DeviceCert, PublicKey, Signed};
use cadrs_social::pairing::{Pairing, PairingOffer, PairingRequest};
use cadrs_ui::Button;
use cadrs_ui::dialog::{Dialog, DialogClose};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::input::{TextInput, TextInputField};
use cadrs_ui::theme::Theme;

use cadrs_social::post::Post;

use crate::social::Social;

/// This device offered to link another: the offer, and the request that answered it.
#[derive(Resource)]
struct Linking {
    pairing: Pairing,
    request: Option<PairingRequest>,
}

/// The code the open *Link a device…* shows.
#[derive(Resource)]
pub struct OfferText(pub String);

/// The code both screens show, once a device asked to join.
#[derive(Resource)]
pub struct ShownCode(pub String);

/// This device asked to join an account.
#[derive(Resource)]
struct Joining {
    offer: PairingOffer,
}

#[derive(Component)]
struct PairDialog;

fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<PairDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
}

fn spawn(
    world: &mut World,
    name: &'static str,
    title: String,
    body: Vec<Line>,
    ok: Option<OkButton>,
) {
    close(world);
    let theme = world.resource::<Theme>().clone();
    let t = theme.clone();
    let tb = theme.clone();
    world.spawn((
        Dialog::new(name)
            .title(title)
            .width(520.0)
            .body(move |b| {
                let t = &tb;
                for line in body {
                    match line {
                        Line::Text(text) => {
                            b.spawn(t.paragraph(text, t.font_base, t.foreground));
                        }
                        Line::Code(code) => {
                            b.spawn((
                                Name::new("pair-code"),
                                t.text(code, 32.0, FontWeight::BOLD, t.foreground),
                            ));
                        }
                        Line::Qr(handle) => {
                            b.spawn((
                                Name::new("pair-qr"),
                                ImageNode::new(handle),
                                Node {
                                    width: Val::Px(240.0),
                                    height: Val::Px(240.0),
                                    ..default()
                                },
                            ));
                        }
                        Line::Selectable(text) => {
                            b.spawn(Node {
                                width: Val::Percent(100.0),
                                ..default()
                            })
                            .with_child(
                                cadrs_ui::selectable_text::SelectableText::new("pair-text")
                                    .value(text)
                                    .font(t.font_xs, FontWeight::NORMAL)
                                    .copy_button()
                                    .build(t),
                            );
                        }
                        Line::Input => {
                            b.spawn(
                                TextInput::new("pair-input")
                                    .placeholder(tr("pair-input-placeholder"))
                                    .autofocus()
                                    .build(t),
                            );
                        }
                    }
                }
            })
            .footer(move |f| {
                let t = f.world().resource::<Theme>().clone();
                if let Some((button, label, ok)) = ok {
                    f.spawn((
                        Button::new(button).label(label).primary().build(&t),
                        observe(move |_: On<Activate>, mut commands: Commands| commands.queue(ok)),
                    ));
                }
                f.spawn((
                    Button::new("pair-cancel")
                        .label(tr("common-cancel"))
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            w.remove_resource::<Linking>();
                            w.remove_resource::<Joining>();
                            close(w);
                        })
                    }),
                ));
            })
            .build(&t),
        PairDialog,
    ));
}

/// A dialog's main button: its name, its label and what it does.
type OkButton = (&'static str, String, fn(&mut World));

enum Line {
    Text(String),
    Code(String),
    Qr(Handle<Image>),
    Selectable(String),
    Input,
}

/// *Link a device…*: shows this account's one-time offer.
pub fn open_link(world: &mut World) {
    crate::social::ensure_loaded(world);
    let Some(keys) = world.resource::<Social>().keys.clone() else {
        crate::linked::toast(world, tr("social-no-keys"));
        return;
    };
    let Some(account) = keys.account.as_ref() else {
        crate::linked::toast(world, tr("pair-not-the-account"));
        return;
    };
    // The answer comes over the network.
    if !crate::network::connect(world) {
        return;
    }
    let offer = PairingOffer::new(
        account.id(),
        keys.device_id().0.to_vec(),
        cadrs_social::wall_millis(),
    );
    let text = offer.to_text();
    world.insert_resource(OfferText(text.clone()));
    // No images without a renderer (a headless test): the text alone.
    let qr = cadrs_ui::qr::image(&text).and_then(|i| {
        world
            .get_resource_mut::<Assets<Image>>()
            .map(|mut images| images.add(i))
    });
    world.insert_resource(Linking {
        pairing: Pairing::new(offer),
        request: None,
    });
    let mut body = vec![Line::Text(tr("pair-link-help"))];
    if let Some(qr) = qr {
        body.push(Line::Qr(qr));
    }
    body.push(Line::Selectable(text));
    spawn(world, "pair-link-dialog", tr("pair-link"), body, None);
}

/// A device answered our offer: show the code to compare.
pub(crate) fn requested(world: &mut World, request: PairingRequest) -> Result<(), String> {
    let now = cadrs_social::wall_millis();
    let Some(mut linking) = world.get_resource_mut::<Linking>() else {
        return Err("no device is being linked here".into());
    };
    let code = linking
        .pairing
        .check(&request, now)
        .map_err(|e| format!("{e:?}"))?;
    let name = request.name().to_owned();
    linking.request = Some(request);
    world.insert_resource(ShownCode(code.clone()));
    spawn(
        world,
        "pair-link-dialog",
        tr("pair-link"),
        vec![
            Line::Text(tr_with("pair-confirm-help", &[("name", &name)])),
            Line::Code(code),
        ],
        Some(("pair-confirm", tr("pair-confirm"), confirm)),
    );
    Ok(())
}

/// The owner confirmed the codes match: certify the device and send it its certificate.
pub fn confirm(world: &mut World) {
    let Some(mut linking) = world.remove_resource::<Linking>() else {
        return;
    };
    let Some(request) = linking.request.take() else {
        return;
    };
    let Some(account) = world
        .resource::<Social>()
        .keys
        .as_ref()
        .and_then(|k| k.account.clone())
    else {
        return;
    };
    match linking
        .pairing
        .confirm(&account, &request, cadrs_social::wall_millis())
    {
        Ok(cert) => {
            let device = request.device();
            crate::social::add_linked(world, device);
            // The account's shops come along to the new device.
            crate::org_sync::invite_linked(world, device);
            let what = tr_with("pair-what-grant", &[("name", request.name())]);
            crate::social::send_post(world, &[device], &Post::Grant(cert), what);
            close(world);
        }
        Err(e) => crate::linked::toast(world, format!("{e:?}")),
    }
}

/// *Join my account…*: takes another device's offer.
pub fn open_join(world: &mut World) {
    spawn(
        world,
        "pair-join-dialog",
        tr("pair-join"),
        vec![Line::Text(tr("pair-join-help")), Line::Input],
        Some(("pair-join-send", tr("pair-join-send"), join)),
    );
}

fn join(world: &mut World) {
    let text = {
        let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
        q.iter(world)
            .find(|(n, _)| n.as_str() == "pair-input-field")
            .map(|(_, t)| t.value().to_string())
            .unwrap_or_default()
    };
    join_with(world, &text);
}

/// Asks to join the account whose offer is `text` (a scanned or pasted code).
pub fn join_with(world: &mut World, text: &str) {
    let offer = match PairingOffer::from_text(text) {
        Ok(o) => o,
        Err(_) => {
            crate::linked::toast(world, tr("pair-bad-code"));
            return;
        }
    };
    crate::social::ensure_loaded(world);
    let Some(keys) = world.resource::<Social>().keys.clone() else {
        crate::linked::toast(world, tr("social-no-keys"));
        return;
    };
    let name = crate::social::my_name(world);
    let request = match offer.request(&keys.device, &name, cadrs_social::wall_millis()) {
        Ok(r) => r,
        Err(_) => {
            crate::linked::toast(world, tr("pair-expired"));
            return;
        }
    };
    let Ok(address) = <[u8; 32]>::try_from(offer.address.as_slice()) else {
        crate::linked::toast(world, tr("pair-bad-code"));
        return;
    };
    let code = offer.code(&keys.device_id());
    if !crate::social::send_post(
        world,
        &[PublicKey(address)],
        &Post::Pair(request),
        tr("pair-what-request"),
    ) {
        return;
    }
    world.insert_resource(Joining { offer });
    world.insert_resource(ShownCode(code.clone()));
    spawn(
        world,
        "pair-join-dialog",
        tr("pair-join"),
        vec![Line::Text(tr("pair-join-code-help")), Line::Code(code)],
        None,
    );
}

/// The account sent this device its certificate: it joins the account.
pub(crate) fn granted(world: &mut World, cert: Signed<DeviceCert>) -> Result<(), String> {
    let Some(joining) = world.get_resource::<Joining>() else {
        return Err("this device didn't ask to join".into());
    };
    let social = world.resource::<Social>();
    let (Some(mut keys), key_path) = (social.keys.clone(), social.key_path.clone()) else {
        return Err("no keys".into());
    };
    joining
        .offer
        .accept(&keys.device_id(), &cert)
        .map_err(|e| format!("{e:?}"))?;
    keys.join(&key_path, cert).map_err(|e| e.to_string())?;
    world.resource_mut::<Social>().keys = Some(keys);
    world.remove_resource::<Joining>();
    close(world);
    crate::linked::toast(world, tr("pair-joined"));
    Ok(())
}
