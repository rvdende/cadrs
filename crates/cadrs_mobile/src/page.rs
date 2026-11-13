//! The phone page: its screens (plain HTML, drawn again after each change) and its network.
//!
//! One click listener on the page reads each button's `data-action`; inputs are read by id.
//! Posts that arrive are handed to [`Phone::handle`]; letters go to the shop's devices, and to
//! its mailboxes sealed when none is online. The phone collects from its mailboxes as it starts
//! and every few minutes after.

use std::cell::RefCell;
use std::rc::Rc;

use cadrs_org::Stage;
use cadrs_org::letters::Composed;
use cadrs_social::contact::Card;
use cadrs_social::keys::{DeviceId, PublicKey};
use cadrs_social::post::Post;
use cadrs_social_net::Node;
use cadrs_social_net::iroh::{EndpointAddr, EndpointId};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{Document, HtmlElement, HtmlInputElement};

use crate::state::Phone;

/// Where the phone keeps its files (in the page's IndexedDB).
const ROOT: &str = "/mobile";
/// The public seed's cadrs ID (`cadrs_net::public::DEFAULT_SEEDS`): the default mailbox.
const SEEDS: &[&str] = &["a560c0cfdeaac54cde945476f851486bbdf584adc280c4120616b06a6b10ba6f"];
const COLLECT_EVERY_MS: i32 = 5 * 60 * 1000;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Card,
    Requests,
    Contacts,
    Notices,
    Shop,
}

struct App {
    phone: Phone,
    node: Option<Node>,
    tab: Tab,
    /// The request whose messages are open.
    chat: Option<cadrs_org::Id>,
    /// The code to compare while joining an account.
    code: Option<String>,
    /// The camera is looking for a QR code.
    scanning: bool,
    status: String,
}

type Shared = Rc<RefCell<App>>;

fn now() -> u64 {
    js_sys::Date::now() as u64
}

fn document() -> Document {
    web_sys::window()
        .and_then(|w| w.document())
        .expect("a page")
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn input(id: &str) -> String {
    document()
        .get_element_by_id(id)
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
        .map(|i| i.value())
        .unwrap_or_default()
}

fn hex32(text: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

fn addr(device: &DeviceId) -> Option<EndpointAddr> {
    EndpointId::from_bytes(&device.0)
        .ok()
        .map(EndpointAddr::new)
}

#[wasm_bindgen(start)]
pub fn start() {
    let mailboxes: Vec<DeviceId> = SEEDS
        .iter()
        .filter_map(|s| hex32(s))
        .map(PublicKey)
        .collect();
    let phone = match Phone::load(std::path::Path::new(ROOT), mailboxes, now()) {
        Ok(p) => p,
        Err(e) => {
            if let Some(root) = document().get_element_by_id("app") {
                root.set_inner_html(&format!(
                    "<p>cadrs can't start: {}</p>",
                    esc(&e.to_string())
                ));
            }
            return;
        }
    };
    let location = web_sys::window().map(|w| w.location());
    let href = location
        .as_ref()
        .and_then(|l| l.href().ok())
        .unwrap_or_default();
    let hash = location
        .as_ref()
        .and_then(|l| l.hash().ok())
        .unwrap_or_default();
    let mut phone = phone;
    let mut status = "Connecting…".to_string();
    let mut tab = match hash.as_str() {
        "#me" => Tab::Card,
        "#contacts" => Tab::Contacts,
        "#news" => Tab::Notices,
        "#shop" => Tab::Shop,
        _ => Tab::Requests,
    };
    // Opened from someone's card (its QR code, a link): add them.
    if let Some(at) = href.find("/c/") {
        let link = format!("{}{}", cadrs_social::contact::LINK_PREFIX, &href[at + 3..]);
        let link = link.split(['#', '?']).next().unwrap_or_default().to_owned();
        status = match phone.add_contact(&link) {
            Ok(name) => format!("{name} is in your contacts"),
            Err(e) => e,
        };
        tab = Tab::Contacts;
    }
    let app: Shared = Rc::new(RefCell::new(App {
        phone,
        node: None,
        tab,
        chat: None,
        code: None,
        scanning: false,
        status,
    }));
    listen(app.clone());
    render(&app.borrow());
    // `#scan`: straight to the camera.
    if hash == "#scan" {
        act(&app, "scan");
    }
    wasm_bindgen_futures::spawn_local(go_online(app));
}

async fn go_online(app: Shared) {
    let secret = app.borrow().phone.keys.device.to_bytes();
    match Node::spawn(secret, true).await {
        Ok((node, mut incoming)) => {
            {
                let mut a = app.borrow_mut();
                a.node = Some(node);
                if a.status.starts_with("Connecting") {
                    a.status = "Online".into();
                }
                render(&a);
            }
            collect(app.clone());
            // Collect now and then.
            let again = app.clone();
            let tick = Closure::<dyn FnMut()>::new(move || collect(again.clone()));
            if let Some(w) = web_sys::window() {
                let _ = w.set_interval_with_callback_and_timeout_and_arguments_0(
                    tick.as_ref().unchecked_ref(),
                    COLLECT_EVERY_MS,
                );
            }
            tick.forget();
            while let Some(post) = incoming.recv().await {
                let from = PublicKey(*post.from.as_bytes());
                let answer = match Post::decode(&post.bytes) {
                    Some(p) => app.borrow_mut().phone.handle(p, Some(from), now()),
                    None => Err("not something cadrs sends".into()),
                };
                match answer {
                    Ok(bytes) => post.take(bytes),
                    Err(why) => post.refuse(why),
                }
                render(&app.borrow());
            }
        }
        Err(e) => {
            let mut a = app.borrow_mut();
            a.status = format!("Offline: {e}");
            render(&a);
        }
    }
}

/// Asks the shops' other member devices for what this phone lacks, and sends back what they
/// lack.
fn sync_shops(app: Shared) {
    let (node, posts) = {
        let a = app.borrow();
        let p = &a.phone;
        (
            a.node.clone(),
            p.shops.sync_posts(p.keys.device_id(), &p.keys.cert),
        )
    };
    let Some(node) = node else { return };
    for (log, device, post) in posts {
        let (node, app) = (node.clone(), app.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let Some(to) = addr(&device) else { return };
            let Ok(answer) = node.send(to.clone(), &post).await else {
                return;
            };
            let back = {
                let mut a = app.borrow_mut();
                let back = a.phone.shops.synced(&log, &answer, now());
                a.phone.save();
                render(&a);
                back
            };
            if let Some(back) = back {
                let _ = node.send(to, &back).await;
            }
        });
    }
}

/// Collects what the mailboxes kept for this phone, and syncs its shops.
fn collect(app: Shared) {
    sync_shops(app.clone());
    let (node, posts) = {
        let a = app.borrow();
        (a.node.clone(), a.phone.collect_posts(now()))
    };
    let Some(node) = node else { return };
    for (mailbox, post) in posts {
        let (node, app) = (node.clone(), app.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let Some(to) = addr(&mailbox) else { return };
            if let Ok(answer) = node.send(to, &post).await {
                app.borrow_mut().phone.take_collected(&answer, now());
                render(&app.borrow());
            }
        });
    }
}

/// Sends a letter: to the shop's devices, else sealed to its mailboxes.
fn send(app: &Shared, to: Card, composed: Composed, what: &'static str) {
    let Some(node) = app.borrow().node.clone() else {
        app.borrow_mut().status = "Offline: it will go when you're back online".into();
        return;
    };
    let app = app.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let mut delivered = false;
        for device in &to.devices {
            if let Some(a) = addr(device)
                && node.send(a, &composed.post).await.is_ok()
            {
                delivered = true;
                break;
            }
        }
        if !delivered {
            'envelopes: for envelope in composed.envelopes {
                let post = Post::Deposit(envelope).encode();
                for mailbox in &to.mailboxes {
                    if let Some(a) = addr(mailbox)
                        && node.send(a, &post).await.is_ok()
                    {
                        continue 'envelopes;
                    }
                }
            }
        }
        let mut a = app.borrow_mut();
        a.status = if delivered {
            format!("{what} delivered")
        } else {
            format!("{what} will be delivered when they come online")
        };
        render(&a);
    });
}

/// The one click listener: buttons say what they do in `data-action`.
fn listen(app: Shared) {
    let on_click = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        let Some(target) = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let Ok(Some(button)) = target.closest("[data-action]") else {
            return;
        };
        let action = button.get_attribute("data-action").unwrap_or_default();
        act(&app, &action);
        render(&app.borrow());
    });
    document()
        .add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())
        .expect("a page takes clicks");
    on_click.forget();
}

fn act(app: &Shared, action: &str) {
    let (verb, arg) = action.split_once(':').unwrap_or((action, ""));
    let id = arg.parse::<cadrs_org::Id>().ok();
    match verb {
        "tab" => {
            let mut a = app.borrow_mut();
            a.chat = None;
            a.tab = match arg {
                "card" => Tab::Card,
                "contacts" => Tab::Contacts,
                "notices" => Tab::Notices,
                "shop" => Tab::Shop,
                _ => Tab::Requests,
            };
        }
        "name" => app.borrow_mut().phone.set_name(&input("name")),
        "contact" => add_contact(app, &input("link")),
        "join" => join(app, &input("pair")),
        "scan" => {
            if !app.borrow().scanning {
                app.borrow_mut().scanning = true;
                wasm_bindgen_futures::spawn_local(scan(app.clone()));
            }
        }
        "scan-stop" => app.borrow_mut().scanning = false,
        "accept" => {
            let Some(id) = id else { return };
            let out = app.borrow_mut().phone.accept(id, now());
            match out {
                Ok((card, composed)) => {
                    send(app, card, composed, "Your acceptance");
                    share(app);
                }
                Err(e) => app.borrow_mut().status = e,
            }
        }
        "chat" => app.borrow_mut().chat = id,
        "quote" | "decline" | "produce" | "ready" => {
            // `verb:<shop>:<request>`.
            let Some((shop, request)) = arg.split_once(':').and_then(|(s, r)| {
                Some((s.parse::<usize>().ok()?, r.parse::<cadrs_org::Id>().ok()?))
            }) else {
                return;
            };
            let out = {
                let mut a = app.borrow_mut();
                let key = a.phone.keys.device.clone();
                let shops = &mut a.phone.shops;
                let out = match verb {
                    "quote" => shops.quote(shop, request, &key, now()),
                    "decline" => shops.decline(shop, request, &key, now()),
                    "produce" => shops.advance(shop, request, Stage::InProduction, &key, now()),
                    _ => shops.advance(shop, request, Stage::Ready, &key, now()),
                };
                a.phone.save();
                out
            };
            match out {
                Ok((card, composed)) => {
                    send(app, card, composed, "The answer");
                    sync_shops(app.clone());
                }
                Err(e) => app.borrow_mut().status = e,
            }
        }
        "say" => {
            let Some(id) = id else { return };
            let text = input("say");
            let out = app.borrow_mut().phone.say(id, &text, now());
            match out {
                Ok((card, composed)) => {
                    send(app, card, composed, "Your message");
                    share(app);
                }
                Err(e) => app.borrow_mut().status = e,
            }
        }
        _ => {}
    }
}

/// Shares the phone's book of requests with the account's other devices, quietly.
fn share(app: &Shared) {
    let (node, post, siblings) = {
        let a = app.borrow();
        (
            a.node.clone(),
            a.phone.mirror_post(),
            a.phone.siblings.clone(),
        )
    };
    let Some(node) = node else { return };
    for device in siblings {
        let (node, post) = (node.clone(), post.clone());
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(to) = addr(&device) {
                let _ = node.send(to, &post).await;
            }
        });
    }
}

fn add_contact(app: &Shared, link: &str) {
    let mut a = app.borrow_mut();
    a.status = match a.phone.add_contact(link) {
        Ok(name) => format!(
            "{} is in your contacts",
            if name.is_empty() { "Someone" } else { &name }
        ),
        Err(e) => e,
    };
    a.tab = Tab::Contacts;
}

/// Asks to join the account whose pairing code is `text`.
fn join(app: &Shared, text: &str) {
    let joined = app.borrow_mut().phone.join(text, now());
    match joined {
        Ok((to, post, code)) => {
            let node = app.borrow().node.clone();
            {
                let mut a = app.borrow_mut();
                a.code = Some(code);
                a.tab = Tab::Card;
            }
            if let (Some(node), Some(to)) = (node, addr(&to)) {
                let app = app.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let result = node.send(to, &post).await;
                    let mut a = app.borrow_mut();
                    a.status = match result {
                        Ok(_) => "Confirm the code on your other device".into(),
                        Err(e) => e,
                    };
                    render(&a);
                });
            }
        }
        Err(e) => app.borrow_mut().status = e,
    }
}

/// Waits `ms` milliseconds.
async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Looks through the back camera for a QR code: a card's link adds the contact, a pairing code
/// asks to join the account.
async fn scan(app: Shared) {
    let doc = document();
    let shown = |on: bool| {
        if let Some(panel) = doc.get_element_by_id("scanner") {
            let _ = if on {
                panel.remove_attribute("hidden")
            } else {
                panel.set_attribute("hidden", "")
            };
        }
    };
    let stream = async {
        let devices = web_sys::window()?.navigator().media_devices().ok()?;
        let constraints = web_sys::MediaStreamConstraints::new();
        let video = js_sys::Object::new();
        js_sys::Reflect::set(&video, &"facingMode".into(), &"environment".into()).ok()?;
        constraints.set_video(&video);
        let promise = devices.get_user_media_with_constraints(&constraints).ok()?;
        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .ok()?
            .dyn_into::<web_sys::MediaStream>()
            .ok()
    }
    .await;
    let Some(stream) = stream else {
        let mut a = app.borrow_mut();
        a.scanning = false;
        a.status = "No camera (or it wasn't allowed)".into();
        render(&a);
        return;
    };
    let video = doc
        .get_element_by_id("cam")
        .and_then(|e| e.dyn_into::<web_sys::HtmlVideoElement>().ok());
    let canvas = doc
        .create_element("canvas")
        .ok()
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
    let (Some(video), Some(canvas)) = (video, canvas) else {
        let mut a = app.borrow_mut();
        a.scanning = false;
        a.status = "This browser can't show the camera here".into();
        render(&a);
        return;
    };
    video.set_src_object(Some(&stream));
    shown(true);
    if let Ok(p) = video.play() {
        let _ = wasm_bindgen_futures::JsFuture::from(p).await;
    }
    let ctx = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok());
    let mut found = None;
    let mut trouble = None;
    let mut frames = 0u32;
    while app.borrow().scanning && found.is_none() {
        sleep(250).await;
        let (vw, vh) = (video.video_width(), video.video_height());
        let Some(ctx) = ctx.as_ref() else {
            trouble = Some("This browser can't read the camera's pictures".to_string());
            break;
        };
        if vw == 0 || vh == 0 {
            continue;
        }
        // At most 640 wide: plenty for a code that fills some of the view, and quick.
        let scale = (640.0 / vw as f64).min(1.0);
        let (w, h) = ((vw as f64 * scale) as u32, (vh as f64 * scale) as u32);
        canvas.set_width(w);
        canvas.set_height(h);
        if ctx
            .draw_image_with_html_video_element_and_dw_and_dh(&video, 0.0, 0.0, w as f64, h as f64)
            .is_err()
        {
            continue;
        }
        match ctx.get_image_data(0.0, 0.0, w as f64, h as f64) {
            Ok(data) => {
                frames += 1;
                found = crate::scan::read(&data.data(), w as usize, h as usize);
            }
            Err(e) => {
                trouble = Some(format!("Can't read the camera: {e:?}"));
                break;
            }
        }
    }
    let tracks = stream.get_tracks();
    for i in 0..tracks.length() {
        if let Ok(track) = tracks.get(i).dyn_into::<web_sys::MediaStreamTrack>() {
            track.stop();
        }
    }
    video.set_src_object(None);
    shown(false);
    app.borrow_mut().scanning = false;
    match (found, trouble) {
        (Some(text), _) => {
            if text.trim_start().starts_with(cadrs_social::pairing::PREFIX) {
                join(&app, &text);
            } else {
                add_contact(&app, &text);
            }
        }
        (None, Some(why)) => app.borrow_mut().status = why,
        (None, None) => {
            app.borrow_mut().status = format!("No code found ({frames} pictures looked at)")
        }
    }
    render(&app.borrow());
}

fn stage(stage: Stage) -> &'static str {
    match stage {
        Stage::Requested => "Requested",
        Stage::Quoted => "Quoted",
        Stage::Declined => "Declined",
        Stage::Accepted => "Ordered",
        Stage::InProduction => "In production",
        Stage::Ready => "Ready",
        Stage::Completed => "Completed",
    }
}

fn render(app: &App) {
    let Some(root) = document().get_element_by_id("app") else {
        return;
    };
    let phone = &app.phone;
    let tab = |t: Tab, key: &str, label: &str| {
        format!(
            r#"<button class="tab{}" data-action="tab:{key}">{label}</button>"#,
            if app.tab == t { " on" } else { "" }
        )
    };
    let shop_tab = if phone.shops.list.is_empty() {
        String::new()
    } else {
        tab(Tab::Shop, "shop", "Shop")
    };
    let mut html = format!(
        r#"<header><h1>cadrs</h1><span class="status">{}</span></header><nav>{}{}{}{}{}</nav><main>"#,
        esc(&app.status),
        tab(Tab::Requests, "requests", "Requests"),
        shop_tab,
        tab(Tab::Contacts, "contacts", "Contacts"),
        tab(Tab::Notices, "notices", "News"),
        tab(Tab::Card, "card", "Me"),
    );
    match (app.tab, app.chat) {
        (_, Some(id)) => chat(&mut html, app, id),
        (Tab::Requests, None) => {
            if phone.book.sent.is_empty() {
                html += "<p class=muted>The shops' answers to your requests for quotes show here. Ask for quotes in cadrs on a computer: Request a quote… in a document's menu.</p>";
            }
            for (id, s) in phone.book.sent.iter().rev() {
                let parts: Vec<String> = s
                    .parts
                    .iter()
                    .map(|(p, n)| format!("{n} × {}", p.name))
                    .collect();
                let price = s
                    .quote
                    .as_ref()
                    .map(|q| format!("{} {}", q.currency, q.total.format()))
                    .unwrap_or_default();
                html += &format!(
                    r#"<section class=card><b>{}</b><div>{}</div><div class=row><span class=stage>{}</span><span>{}</span></div><div class=row>"#,
                    esc(&s.shop_name),
                    esc(&parts.join(", ")),
                    stage(s.stage),
                    esc(&price),
                );
                if s.stage == Stage::Quoted {
                    html += &format!(
                        r#"<button class=primary data-action="accept:{id}">Accept</button>"#
                    );
                }
                html += &format!(
                    r#"<button data-action="chat:{id}">Messages ({})</button></div></section>"#,
                    s.chat.len()
                );
            }
        }
        (Tab::Contacts, None) => {
            for c in &phone.contacts.cards {
                let name = if c.value.name.is_empty() {
                    "Someone"
                } else {
                    &c.value.name
                };
                let kind = match c.value.kind {
                    cadrs_social::contact::Kind::Organization => "a shop",
                    cadrs_social::contact::Kind::Person => "a person",
                };
                html += &format!(
                    "<section class=card><b>{}</b> <span class=muted>{kind}</span></section>",
                    esc(name)
                );
            }
            html += r#"<p><button class=primary data-action="scan">Scan a code</button></p><p><input id=link placeholder="Or paste a link: https://cadrs.net/c/…"><button data-action="contact">Add contact</button></p>"#;
        }
        (Tab::Shop, None) => shop_view(&mut html, phone),
        (Tab::Notices, None) => {
            if phone.notices.is_empty() {
                html += "<p class=muted>Nothing new.</p>";
            }
            for n in &phone.notices {
                html += &format!("<section class=card>{}</section>", esc(&n.text));
            }
        }
        (Tab::Card, None) => {
            html += &format!(
                r#"<p><input id=name value="{}" placeholder="Your name"><button data-action="name">Save</button></p>"#,
                esc(&phone.name)
            );
            if let Some(card) = phone.card(now()) {
                let link = cadrs_social::contact::to_link(&card);
                let svg = qrcode::QrCode::new(link.as_bytes())
                    .map(|c| {
                        c.render::<qrcode::render::svg::Color>()
                            .min_dimensions(240, 240)
                            .build()
                    })
                    .unwrap_or_default();
                html += &format!(
                    r#"<p>Show this to add each other:</p><div class=qr>{svg}</div><p class=link>{}</p>"#,
                    esc(&link)
                );
            } else {
                html +=
                    "<p>This phone belongs to your account; your card is on your main device.</p>";
            }
            if !phone.joined() {
                html += r#"<h2>Join my account</h2><p class=muted>On your computer: Link a device… in the account menu. Paste its code here.</p><p><button class=primary data-action="scan">Scan its code</button></p><p><input id=pair placeholder="Or paste it: cadrs-pair:…"><button data-action="join">Join</button></p>"#;
                if let Some(code) = &app.code {
                    html += &format!(
                        "<p>Check your computer shows <b class=code>{}</b></p>",
                        esc(code)
                    );
                }
            }
        }
    }
    html += "</main>";
    root.set_inner_html(&html);
    if let Some(main) = root
        .query_selector("main")
        .ok()
        .flatten()
        .and_then(|m| m.dyn_into::<HtmlElement>().ok())
    {
        main.set_scroll_top(0);
    }
}

/// The shops' requests and orders, newest first, with what can be done next.
fn shop_view(html: &mut String, phone: &Phone) {
    for (i, shop) in phone.shops.list.iter().enumerate() {
        let org = shop.session.org();
        *html += &format!("<h2>{}</h2>", esc(&shop.info.name));
        if org.org.is_none() {
            *html += "<p class=muted>Syncing with the shop's other devices…</p>";
        }
        if shop.info.cert.is_none() {
            *html += "<p class=muted>This phone sees the shop's requests; answering them needs the device that holds the shop's key to link it.</p>";
        }
        for (id, r) in org.requests.iter().rev() {
            let client = r.card.as_ref().map_or_else(
                || r.client.to_string()[..8].to_string(),
                |c| c.value.name.clone(),
            );
            let parts: Vec<String> = r
                .parts
                .iter()
                .map(|(p, n)| format!("{n} × {}", p.name))
                .collect();
            let price = r
                .quote
                .as_ref()
                .map(|q| format!("{} {}", q.currency, q.total.format()))
                .unwrap_or_default();
            *html += &format!(
                r#"<section class=card><b>{}</b><div>{}</div><div class=row><span class=stage>{}</span><span>{}</span></div><div class=row>"#,
                esc(&client),
                esc(&parts.join(", ")),
                stage(r.stage),
                esc(&price)
            );
            let buttons: &[(&str, &str, bool)] = match r.stage {
                Stage::Requested => &[("quote", "Quote", true), ("decline", "Decline", false)],
                Stage::Accepted => &[("produce", "Start production", true)],
                Stage::InProduction => &[("ready", "Ready", true)],
                _ => &[],
            };
            for (verb, label, primary) in buttons {
                *html += &format!(
                    r#"<button{} data-action="{verb}:{i}:{id}">{label}</button>"#,
                    if *primary { " class=primary" } else { "" }
                );
            }
            *html += "</div></section>";
        }
        if org.requests.is_empty() && org.org.is_some() {
            *html += "<p class=muted>No requests for quotes yet.</p>";
        }
    }
}

fn chat(html: &mut String, app: &App, id: cadrs_org::Id) {
    let Some(s) = app.phone.book.sent.get(&id) else {
        return;
    };
    *html += &format!(
        r#"<button data-action="tab:requests">‹ Requests</button><h2>{}</h2>"#,
        esc(&s.shop_name)
    );
    for (from_shop, text, _) in &s.chat {
        *html += &format!(
            "<div class=\"msg {}\">{}</div>",
            if *from_shop { "them" } else { "me" },
            esc(text)
        );
    }
    *html += &format!(
        r#"<p><input id=say placeholder="Write a message"><button class=primary data-action="say:{id}">Send</button></p>"#
    );
}
