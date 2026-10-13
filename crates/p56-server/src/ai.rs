//! AI pictures: a prompt becomes a still, an object turning round its own axis, or a short
//! animation for the fan. They are made by ComfyUI on a computer in the house (`--comfyui`), or
//! through Home Assistant's AI Task (only in the add-on), by whatever AI the user set up there
//! (Google, OpenAI, …). No key or account is kept here. With ComfyUI an object can also be made
//! as a real 3D model (Trellis 2) and turned round on all sides.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use image::RgbImage;
use p56_core::transcode;
use serde_json::{json, Value};

/// `AITaskEntityFeature.GENERATE_IMAGE` in Home Assistant.
const GENERATE_IMAGE: u64 = 4;
/// How long one picture may take (the AI services answer in 5–30 s).
const GENERATE_TIMEOUT: Duration = Duration::from_secs(120);
/// Frames of a turning object: one turn round its own upright axis in 2.4 s.
const SPIN_FRAMES: u32 = 24;
const SPIN_MS: u32 = 100;
/// A drawn animation: a 4×4 sheet of frames.
const SHEET: u32 = 4;
const SHEET_MS: u32 = 125;
/// A real 3D model: views round it, 6° apart (24 looked like a slideshow), one turn in 6 s (3.6 s
/// turned too fast on the real fan), and
/// how long making one may take (about 2 minutes on a laptop graphics card).
const REAL3D_VIEWS: u32 = 60;
const REAL3D_MS: u32 = 100;
const REAL3D_SIZE: u32 = 384;
const REAL3D_TIMEOUT: Duration = Duration::from_secs(600);

/// What to make of the prompt.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// One picture.
    Still,
    /// One picture, turned round its own upright axis by the server, as on a turntable (the AI
    /// cannot draw the views of a turn consistently).
    Spin,
    /// 16 frames drawn by the AI (a sprite sheet).
    Anim,
    /// A background for text written over it (`show_ai` with `text`): a whole, simple picture of the theme.
    Backdrop,
    /// A real 3D model of the object (ComfyUI with Trellis 2), seen turning round on all sides.
    Real3d,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Kind, String> {
        match s {
            "" | "still" => Ok(Kind::Still),
            "spin" => Ok(Kind::Spin),
            "anim" | "animation" => Ok(Kind::Anim),
            "3d" | "real3d" => Ok(Kind::Real3d),
            _ => Err(format!("unknown kind '{s}' (still, spin, 3d or anim)")),
        }
    }
}

/// What the fan needs, added to every prompt. Only technical: the style is the user's. It does
/// not mention the fan itself: some models (Qwen Image) then paint a fan or a ring of light.
const LOOK: &str = "with the subject alone on a pure black background (#000000) that runs up to every \
edge of the image: no white or coloured margin, no floor, no shadow, no frame, border, circle or ring \
around it, no screen or device, no text, letters or numbers unless the subject asks for them. The \
subject itself is brightly lit and clearly visible, in the middle, filling most of the image.";

/// Ready-made styles (`art_style`); any other text is used as the style itself.
const STYLES: [(&str, &str); 10] = [
    ("photo", "a realistic photograph"),
    ("3d", "a detailed 3D render"),
    ("cartoon", "a cartoon"),
    ("anime", "anime"),
    ("painting", "an oil painting"),
    ("watercolor", "a watercolour painting"),
    ("pixel", "pixel art"),
    ("neon", "glowing neon lines"),
    ("line", "clean line art"),
    ("comic", "a comic book drawing"),
];

/// The style line of the instructions: a ready-made style, the user's own words, or nothing
/// (the AI follows the prompt).
fn style_line(style: Option<&str>) -> String {
    match style.map(str::trim).filter(|s| !s.is_empty() && *s != "free") {
        Some(s) => {
            let words = STYLES.iter().find(|(k, _)| k.eq_ignore_ascii_case(s)).map_or_else(|| s.chars().take(80).collect(), |(_, w)| w.to_string());
            format!("\nStyle: {words}")
        }
        None => String::new(),
    }
}

/// The instructions sent to the AI for the user's prompt.
pub fn instructions(prompt: &str, kind: Kind, style: Option<&str>) -> String {
    let prompt = prompt.trim();
    let style = style_line(style);
    let body = match kind {
        Kind::Still => format!("A square image {LOOK}\nSubject: {prompt}"),
        Kind::Spin => format!("A square image, the subject seen straight from the front, {LOOK}\nSubject: {prompt}"),
        // the model is made from this one picture: the whole object, a little from above
        Kind::Real3d => format!("A square image, the whole subject seen from the front and slightly above, {LOOK}\nSubject: {prompt}"),
        // A whole picture: the text is written over it, on a darkened band. The theme sits inside
        // the sentence (at the end Qwen Image drew random things) but not in quotes (then it
        // writes it out), and the background is darker, not dark (dark everywhere shows nothing).
        Kind::Backdrop => format!(
            "A square illustration without any writing that fills the whole image from edge to edge. It shows \
             one simple, bold scene with the things that belong to this situation: {prompt}. Those things are \
             large, bright and vivid, in rich colours, and fill most of the image; around them a darker \
             background. Not busy, little fine detail, no white areas. There are no letters, words, numbers, \
             signs or labels anywhere in the picture."
        ),
        Kind::Anim => format!(
            "A square sprite sheet that fills the whole image on pure black, with no white or coloured margin, \
             frame or border around it: a {SHEET} by {SHEET} grid of {} equal square cells with a little black space \
             between them and no lines or numbers. The cells are the frames of one short looping animation in \
             reading order (left to right, top to bottom), the last frame leading back into the first. Every frame \
             is a clearly different moment of the motion (for example legs, wings, arms or the body in a new \
             position), so that played quickly one after another the subject visibly moves; never the same \
             picture repeated. The subject stays in the middle of its cell at the same size and never travels \
             across it: an object turns once around its own upright axis over the {} frames, as on a turntable; \
             a creature or figure moves on the spot. Each cell is a picture {LOOK}\nSubject: {prompt}",
            SHEET * SHEET,
            SHEET * SHEET
        ),
    };
    body + &style
}

/// The id that picks ComfyUI where an AI Task entity is expected.
pub const COMFYUI: &str = "comfyui";
/// ComfyUI on a computer in the house: its address and the picture size it makes.
pub struct Comfy {
    pub url: String,
    pub size: u32,
}
static COMFY: OnceLock<Comfy> = OnceLock::new();
/// Where real 3D models are kept (their views), so the same one is there at once next time.
static KEEP_3D: OnceLock<PathBuf> = OnceLock::new();

/// Keep the views of real 3D models in `dir` (e.g. /data/ai in the add-on).
pub fn keep_3d_in(dir: PathBuf) {
    let _ = KEEP_3D.set(dir);
}

/// Make pictures with ComfyUI at `url` (`http://host:port`), `size` pixels square (rounded to a
/// multiple of 64, 256–1536).
pub fn use_comfyui(url: &str, size: u32) -> Result<(), String> {
    let url = url.trim().trim_end_matches('/');
    if !url.starts_with("http://") || url.len() <= 7 {
        return Err(format!("ComfyUI address must look like http://192.168.1.20:8188, not {url}"));
    }
    let size = (size.clamp(256, 1536) + 32) / 64 * 64;
    let _ = COMFY.set(Comfy { url: url.to_string(), size });
    Ok(())
}

fn comfy() -> Option<&'static Comfy> {
    COMFY.get()
}

/// The AIs that can make pictures: `[{entity_id, name}]`, ComfyUI first when it is set up.
/// `ha` is Home Assistant's API (base, token), when there is one.
pub fn entities(ha: Option<(&str, &str)>) -> Result<Vec<Value>, String> {
    let mut list = Vec::new();
    if let Some(c) = comfy() {
        list.push(json!({ "entity_id": COMFYUI, "name": format!("ComfyUI ({})", c.url.trim_start_matches("http://")) }));
    }
    match ha.map(|(base, token)| ha_entities(base, token)) {
        Some(Ok(more)) => list.extend(more),
        Some(Err(e)) if list.is_empty() => return Err(e),
        None if list.is_empty() => return Err("AI pictures need ComfyUI (--comfyui) or the Home Assistant add-on".into()),
        _ => {}
    }
    Ok(list)
}

/// The AI Task entities in Home Assistant that can make pictures.
fn ha_entities(base: &str, token: &str) -> Result<Vec<Value>, String> {
    // right after Home Assistant restarts its API can fail for a moment: try once more
    let fetch = || crate::http_get(&format!("{base}/states"), Some(token), "application/json").and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let states = fetch().or_else(|| {
        std::thread::sleep(Duration::from_secs(3));
        fetch()
    });
    let states = states.as_ref().and_then(|v| v.as_array()).ok_or("Home Assistant did not list its entities (is homeassistant_api on?)")?;
    Ok(states
        .iter()
        .filter(|e| e["attributes"]["supported_features"].as_u64().is_some_and(|f| f & GENERATE_IMAGE != 0))
        .filter_map(|e| {
            let id = e["entity_id"].as_str().filter(|id| id.starts_with("ai_task."))?;
            Some(json!({ "entity_id": id, "name": e["attributes"]["friendly_name"].as_str().unwrap_or(id) }))
        })
        .collect())
}

/// Pictures made before, by entity and instructions: an automation that shows the same prompt
/// again (e.g. a price background every quarter of an hour) does not pay for a new picture.
static MADE: std::sync::Mutex<Vec<(String, Vec<RgbImage>, u32)>> = std::sync::Mutex::new(Vec::new());
const MADE_KEPT: usize = 8;

/// Ask the AI for a picture and turn it into frames for the fan: (pictures, ms per frame). The
/// same prompt again gives the same picture, unless `fresh`.
pub fn generate(
    ha: Option<(&str, &str)>, entity: Option<&str>, prompt: &str, kind: Kind, style: Option<&str>, fresh: bool,
) -> Result<(Vec<RgbImage>, u32), String> {
    if prompt.trim().is_empty() {
        return Err("describe the picture first".into());
    }
    let instructions = instructions(prompt, kind, style);
    if kind == Kind::Real3d {
        return real_3d(entity, &instructions, fresh);
    }
    // the AIs to try in turn: the one asked for, or ComfyUI and then Home Assistant's
    let mut sources: Vec<String> = match entity.filter(|e| !e.is_empty()) {
        Some(COMFYUI) if comfy().is_some() => vec![COMFYUI.into()],
        Some(COMFYUI) => return Err("ComfyUI is not set up (add-on option comfyui_url)".into()),
        Some(e) if e.starts_with("ai_task.") && e[8..].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') => vec![e.to_string()],
        Some(e) => return Err(format!("not an AI Task entity: {e}")),
        None => comfy().map(|_| COMFYUI.to_string()).into_iter().collect(),
    };
    let ha_fallback = entity.filter(|e| !e.is_empty()).is_none();
    let mut last_err = String::new();
    let mut tried_ha = false;
    loop {
        let entity = match sources.first() {
            Some(e) => e.clone(),
            None if ha_fallback && !tried_ha => {
                tried_ha = true;
                let (base, token) =
                    ha.ok_or_else(|| if last_err.is_empty() { "AI pictures need ComfyUI or the Home Assistant add-on".into() } else { last_err.clone() })?;
                match ha_entities(base, token)?.first().and_then(|e| e["entity_id"].as_str().map(String::from)) {
                    Some(e) => e,
                    None if !last_err.is_empty() => return Err(last_err),
                    None => return Err("no AI in Home Assistant can make pictures: add one (e.g. Google Generative AI) and its AI Task".into()),
                }
            }
            None => return Err(last_err),
        };
        if !sources.is_empty() {
            sources.remove(0);
        }
        let key = format!("{entity}\n{instructions}");
        if !fresh {
            if let Some((_, images, ms)) = MADE.lock().unwrap().iter().find(|(k, _, _)| *k == key) {
                log::info!("AI picture from {entity}: made before");
                return Ok((images.clone(), *ms));
            }
        }
        let bytes = if entity == COMFYUI {
            comfy_picture(comfy().expect("checked above"), &instructions, kind)
        } else {
            match ha {
                Some((base, token)) => ha_picture(base, token, &entity, &instructions),
                None => Err("Home Assistant's AI needs the Home Assistant add-on".into()),
            }
        };
        match bytes {
            Ok(bytes) => return finish(&entity, &key, &bytes, kind),
            Err(e) => {
                log::warn!("AI picture from {entity} failed: {e}");
                last_err = e;
            }
        }
    }
}

/// A picture from Home Assistant's AI Task `entity`.
fn ha_picture(base: &str, token: &str, entity: &str, instructions: &str) -> Result<Vec<u8>, String> {
    let body = json!({ "task_name": "P56 picture", "entity_id": entity, "instructions": instructions });
    let url = format!("{base}/services/ai_task/generate_image?return_response");
    let (code, answer) =
        crate::http_request("POST", &url, Some(token), "application/json", Some(&serde_json::to_vec(&body).unwrap_or_default()), GENERATE_TIMEOUT)?;
    let answer: Value = serde_json::from_slice(&answer).unwrap_or(Value::Null);
    if code != 200 {
        let msg = answer["message"].as_str().map(String::from).unwrap_or_else(|| format!("HTTP {code}"));
        return Err(format!("the AI made no picture: {msg}"));
    }
    let res = &answer["service_response"];
    let path = res["url"].as_str().ok_or("the AI answered without a picture")?;
    fetch_picture(base, token, path)
}

/// A GET on ComfyUI's API, as JSON.
fn comfy_get(c: &Comfy, path: &str) -> Result<Value, String> {
    let (code, body) = crate::http_request("GET", &format!("{}{path}", c.url), None, "application/json", None, Duration::from_secs(10))
        .map_err(|e| format!("ComfyUI not reachable: {e}"))?;
    if code != 200 {
        return Err(format!("ComfyUI {path}: HTTP {code}"));
    }
    serde_json::from_slice(&body).map_err(|e| format!("ComfyUI {path}: {e}"))
}

/// A model file in ComfyUI's `models/<folder>` that `want` accepts (its name in lower case): one
/// with "turbo" in its name first (8 steps instead of 25). `what` names it in the error.
fn comfy_pick(c: &Comfy, folder: &str, what: &str, want: &dyn Fn(&str) -> bool) -> Result<String, String> {
    let list = comfy_get(c, &format!("/models/{folder}"))?;
    let names: Vec<&str> = list.as_array().map(|a| a.iter().filter_map(|n| n.as_str()).filter(|n| want(&n.to_ascii_lowercase())).collect()).unwrap_or_default();
    names
        .iter()
        .find(|n| n.to_ascii_lowercase().contains("turbo"))
        .or(names.first())
        .map(|n| n.to_string())
        .ok_or_else(|| format!("ComfyUI has no {what} file in models/{folder}"))
}

/// Run a workflow on ComfyUI and wait for it (at most `timeout`): its history entry.
fn comfy_run(c: &Comfy, graph: Value, timeout: Duration) -> Result<Value, String> {
    let body = serde_json::to_vec(&json!({ "prompt": graph, "client_id": "p56-server" })).unwrap_or_default();
    let (code, answer) = crate::http_request("POST", &format!("{}/prompt", c.url), None, "application/json", Some(&body), Duration::from_secs(20))
        .map_err(|e| format!("ComfyUI not reachable: {e}"))?;
    let answer: Value = serde_json::from_slice(&answer).unwrap_or(Value::Null);
    let id = answer["prompt_id"].as_str().filter(|_| code == 200).ok_or_else(|| {
        let why = answer["error"]["message"].as_str().map(String::from).unwrap_or_else(|| format!("HTTP {code}"));
        format!("ComfyUI refused the picture: {why} {}", answer["node_errors"]).trim().to_string()
    })?;
    let deadline = Instant::now() + timeout;
    let done = loop {
        std::thread::sleep(Duration::from_millis(500));
        if let Some(h) = comfy_get(c, &format!("/history/{id}"))?.get(id).cloned() {
            break h;
        }
        if Instant::now() > deadline {
            return Err("ComfyUI took too long".into());
        }
    };
    if done["status"]["status_str"].as_str() != Some("success") {
        return Err(format!("ComfyUI failed: {}", done["status"]["messages"]).chars().take(400).collect());
    }
    Ok(done)
}

/// The picture behind one output image of a finished workflow (`{filename, subfolder, type}`).
fn comfy_view(c: &Comfy, img: &Value) -> Result<Vec<u8>, String> {
    let q = |k: &str| {
        img[k]
            .as_str()
            .unwrap_or_default()
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect::<String>()
    };
    let (code, png) = crate::http_request(
        "GET",
        &format!("{}/view?filename={}&subfolder={}&type={}", c.url, q("filename"), q("subfolder"), q("type")),
        None,
        "image/*",
        None,
        Duration::from_secs(20),
    )?;
    if code != 200 {
        return Err(format!("ComfyUI did not hand over the picture: HTTP {code}"));
    }
    Ok(png)
}

/// The nodes "1"–"8" that draw `instructions` with Qwen Image 2.1, `size` pixels square; the
/// picture is output 0 of node "8".
fn qwen_nodes(c: &Comfy, instructions: &str, size: u32) -> Result<serde_json::Map<String, Value>, String> {
    let unet = comfy_pick(c, "diffusion_models", "Qwen Image 2.1", &|n| n.contains("qwen_image_2.1") || n.contains("qwen-image-2.1"))?;
    let clip = comfy_pick(c, "text_encoders", "Qwen Image 2.1", &|n| n.contains("qwen3vl") || n.contains("qwen3_vl") || n.contains("qwen_2.5_vl"))?;
    let vae = comfy_pick(c, "vae", "Qwen Image 2.1", &|n| n.contains("qwen_image_2.1") || n.contains("qwen-image-2.1"))?;
    let steps = if unet.to_ascii_lowercase().contains("turbo") { 8 } else { 25 };
    let graph = json!({
        "1": { "class_type": "UNETLoader", "inputs": { "unet_name": unet, "weight_dtype": "default" } },
        "2": { "class_type": "QwenImage21Cache", "inputs": { "model": ["1", 0], "device": "auto", "dtype": "default" } },
        "3": { "class_type": "CLIPLoader", "inputs": { "clip_name": clip, "type": "qwen_image", "device": "default" } },
        "4": { "class_type": "VAELoader", "inputs": { "vae_name": vae } },
        "5": { "class_type": "TextEncodeQwenImage21", "inputs": { "clip": ["3", 0], "prompt": instructions, "negative_prompt": "", "resolution": 1024 } },
        "6": { "class_type": "EmptyLatentImage", "inputs": { "width": size, "height": size, "batch_size": 1 } },
        "7": { "class_type": "KSampler", "inputs": { "model": ["2", 0], "positive": ["5", 0], "negative": ["5", 1], "latent_image": ["6", 0],
            "seed": seed(), "steps": steps, "cfg": 1.0, "sampler_name": "euler", "scheduler": "simple", "denoise": 1.0 } },
        "8": { "class_type": "VAEDecode", "inputs": { "samples": ["7", 0], "vae": ["4", 0] } },
    });
    Ok(graph.as_object().cloned().unwrap_or_default())
}

fn seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64 & 0xffff_ffff_ffff).unwrap_or(1)
}

/// A picture from ComfyUI with Qwen Image 2.1 (the turbo model when it is there), in the size set
/// up (an animation sheet at least 1024).
fn comfy_picture(c: &Comfy, instructions: &str, kind: Kind) -> Result<Vec<u8>, String> {
    let size = if kind == Kind::Anim { c.size.max(1024) } else { c.size };
    let mut graph = qwen_nodes(c, instructions, size)?;
    // a preview, not a saved file: nothing piles up on that computer
    graph.insert("9".into(), json!({ "class_type": "PreviewImage", "inputs": { "images": ["8", 0] } }));
    let done = comfy_run(c, Value::Object(graph), GENERATE_TIMEOUT)?;
    let img = done["outputs"]["9"]["images"].get(0).cloned().ok_or("ComfyUI made no picture")?;
    comfy_view(c, &img)
}

/// A real 3D model of the object, as views round it: from the store when it was made before
/// (unless `fresh`), else made by ComfyUI and kept.
fn real_3d(entity: Option<&str>, instructions: &str, fresh: bool) -> Result<(Vec<RgbImage>, u32), String> {
    if entity.is_some_and(|e| !e.is_empty() && e != COMFYUI) {
        return Err("a real 3D model can only be made by ComfyUI".into());
    }
    let c = comfy().ok_or("a real 3D model needs ComfyUI (add-on option comfyui_url)")?;
    let key = format!("{COMFYUI}\n3d\n{instructions}");
    let dir = KEEP_3D.get().map(|d| d.join(format!("{:016x}", fnv(&key))));
    if !fresh {
        if let Some(views) = dir.as_ref().and_then(|d| load_views(d)) {
            log::info!("3D model from the store ({} views)", views.len());
            return Ok((views, REAL3D_MS));
        }
        if let Some((_, images, ms)) = MADE.lock().unwrap().iter().find(|(k, _, _)| *k == key) {
            log::info!("3D model made before");
            return Ok((images.clone(), *ms));
        }
    }
    let started = Instant::now();
    let views = comfy_3d(c, instructions)?;
    log::info!("3D model from comfyui: {} views in {:.0} s", views.len(), started.elapsed().as_secs_f64());
    match &dir {
        Some(d) => save_views(d, &views, &key),
        None => {
            let mut kept = MADE.lock().unwrap();
            kept.retain(|(k, _, _)| !k.starts_with(&format!("{COMFYUI}\n3d\n")));
            kept.push((key, views.clone(), REAL3D_MS));
        }
    }
    Ok((views, REAL3D_MS))
}

/// FNV-1a: a name for a stored 3D model that stays the same across versions.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

fn load_views(dir: &std::path::Path) -> Option<Vec<RgbImage>> {
    let views: Vec<RgbImage> = (0..REAL3D_VIEWS)
        .map(|i| std::fs::read(dir.join(format!("{i:02}.jpg"))).ok().and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgb8()))
        .collect::<Option<Vec<_>>>()?;
    Some(views)
}

fn save_views(dir: &std::path::Path, views: &[RgbImage], key: &str) {
    let saved = std::fs::create_dir_all(dir).and_then(|_| {
        for (i, v) in views.iter().enumerate() {
            std::fs::write(dir.join(format!("{i:02}.jpg")), transcode::jpeg(v, 90))?;
        }
        std::fs::write(dir.join("prompt.txt"), key)
    });
    if let Err(e) = saved {
        log::warn!("3D model not kept in {}: {e}", dir.display());
    }
}

/// Make a real 3D model with ComfyUI: Qwen Image 2.1 draws the object, its background is cut
/// away, Trellis 2 makes a coloured model of it, and that is rendered from all round. Tried on a
/// laptop graphics card: about 2 minutes. The detail upsampling and mesh decimation of the
/// template are left out (they took over ten minutes); a coarse remesh is plenty for the fan.
fn comfy_3d(c: &Comfy, instructions: &str) -> Result<Vec<RgbImage>, String> {
    let trellis = comfy_pick(c, "diffusion_models", "Trellis 2", &|n| n.contains("trellis_2") || n.contains("trellis2"))?;
    let shape_vae = comfy_pick(c, "vae", "Trellis 2 shape VAE", &|n| (n.contains("trellis_2") || n.contains("trellis2")) && n.contains("shape"))?;
    let texture_vae = comfy_pick(c, "vae", "Trellis 2 texture VAE", &|n| (n.contains("trellis_2") || n.contains("trellis2")) && n.contains("texture"))?;
    let dino = comfy_pick(c, "clip_vision", "DINOv3 (Trellis 2)", &|n| n.contains("dino"))?;
    let cutter = comfy_pick(c, "background_removal", "background removal (BiRefNet)", &|_| true)?;
    let s = seed() & 0xffff_ffff;
    let mut g = qwen_nodes(c, instructions, 1024)?;
    let more = json!({
        // the object cut out of its background, in a square
        "20": { "class_type": "LoadBackgroundRemovalModel", "inputs": { "bg_removal_name": cutter } },
        "21": { "class_type": "RemoveBackground", "inputs": { "bg_removal_model": ["20", 0], "image": ["8", 0] } },
        "22": { "class_type": "ImageCropToMask", "inputs": { "images": ["8", 0], "masks": ["21", 0], "width": 1024, "height": 1024,
            "pad_factor": 1.1, "grow_mask": 0, "background": "#000000" } },
        // Trellis 2: structure, shape and colour (the settings of ComfyUI's own template)
        "30": { "class_type": "UNETLoader", "inputs": { "unet_name": trellis, "weight_dtype": "default" } },
        "31": { "class_type": "CLIPVisionLoader", "inputs": { "clip_name": dino } },
        "32": { "class_type": "VAELoader", "inputs": { "vae_name": shape_vae } },
        "33": { "class_type": "VAELoader", "inputs": { "vae_name": texture_vae } },
        "34": { "class_type": "Trellis2Conditioning", "inputs": { "clip_vision_model": ["31", 0], "image": ["22", 0] } },
        "35": { "class_type": "CFGOverride", "inputs": { "model": ["30", 0], "cfg": 1.0, "start_percent": 0.667, "end_percent": 1.0 } },
        "36": { "class_type": "RescaleCFG", "inputs": { "model": ["35", 0], "multiplier": 0.7 } },
        "37": { "class_type": "ModelSamplingSD3", "inputs": { "model": ["36", 0], "shift": 5.0 } },
        "38": { "class_type": "EmptyTrellis2LatentStructure", "inputs": { "batch_size": 1 } },
        "39": { "class_type": "KSampler", "inputs": { "model": ["37", 0], "positive": ["34", 0], "negative": ["34", 1], "latent_image": ["38", 0],
            "seed": s, "steps": 12, "cfg": 7.5, "sampler_name": "euler", "scheduler": "normal", "denoise": 1.0 } },
        "40": { "class_type": "VaeDecodeStructureTrellis2", "inputs": { "samples": ["39", 0], "vae": ["32", 0], "resolution": "32" } },
        "41": { "class_type": "Trellis2ShapeStage", "inputs": { "positive": ["34", 0], "negative": ["34", 1], "voxel": ["40", 0] } },
        "42": { "class_type": "CFGOverride", "inputs": { "model": ["30", 0], "cfg": 1.0, "start_percent": 0.769, "end_percent": 1.0 } },
        "43": { "class_type": "RescaleCFG", "inputs": { "model": ["42", 0], "multiplier": 0.5 } },
        "44": { "class_type": "KSampler", "inputs": { "model": ["43", 0], "positive": ["41", 0], "negative": ["41", 1], "latent_image": ["41", 2],
            "seed": s, "steps": 20, "cfg": 7.5, "sampler_name": "euler", "scheduler": "normal", "denoise": 1.0 } },
        "47": { "class_type": "VaeDecodeShapeTrellis", "inputs": { "samples": ["44", 0], "vae": ["32", 0] } },
        "48": { "class_type": "Trellis2TextureStage", "inputs": { "positive": ["41", 0], "negative": ["41", 1], "shape_latent": ["44", 0] } },
        "49": { "class_type": "KSampler", "inputs": { "model": ["30", 0], "positive": ["48", 0], "negative": ["48", 1], "latent_image": ["48", 2],
            "seed": s + 1, "steps": 12, "cfg": 1.0, "sampler_name": "euler", "scheduler": "normal", "denoise": 1.0 } },
        "50": { "class_type": "VaeDecodeTextureTrellis", "inputs": { "samples": ["49", 0], "vae": ["33", 0], "shape_subdivides": ["47", 1] } },
        // a coarse mesh (about 25,000 faces) renders 60 times in seconds; the full one took minutes
        "51": { "class_type": "RemeshMesh", "inputs": { "mesh": ["47", 0], "resolution": 128, "sign_mode": "udf", "sign_mode.qef": false,
            "sign_mode.drop_inverted_components": false, "sign_mode.drop_enclosed_components": false, "band": 1.0, "project_back": 0.0,
            "fix_poles": false, "smooth_iters": 0, "drop_small_components": 0.01, "precluster_max_verts": 20_000_000 } },
        "52": { "class_type": "MeshSmoothNormals", "inputs": { "mesh": ["51", 0], "crease_angle": 180.0 } },
        "53": { "class_type": "PaintMesh", "inputs": { "mesh": ["52", 0], "voxel_colors": ["50", 0] } },
    });
    g.extend(more.as_object().cloned().unwrap_or_default());
    // views from all round, at the same distance, so the object does not grow or shrink
    for v in 0..REAL3D_VIEWS {
        let yaw = v as f64 * 360.0 / REAL3D_VIEWS as f64;
        g.insert(
            format!("{}", 100 + v),
            json!({ "class_type": "CreateCameraInfo", "inputs": { "mode": "orbit", "mode.yaw": yaw, "mode.pitch": 12.0,
            "mode.distance": 2.4, "target_x": 0.0, "target_y": 0.0, "target_z": 0.0, "roll": 0.0, "fov": 35.0, "zoom": 1.0, "camera_type": "perspective" } }),
        );
        g.insert(
            format!("{}", 200 + v),
            json!({ "class_type": "RenderMesh", "inputs": { "mesh": ["53", 0], "mode": "vertex colors",
            "width": REAL3D_SIZE, "height": REAL3D_SIZE, "background": "#000000", "camera_info": [format!("{}", 100 + v), 0] } }),
        );
        g.insert(format!("{}", 300 + v), json!({ "class_type": "PreviewImage", "inputs": { "images": [format!("{}", 200 + v), 0] } }));
    }
    let done = comfy_run(c, Value::Object(g), REAL3D_TIMEOUT)?;
    (0..REAL3D_VIEWS)
        .map(|v| {
            let img = done["outputs"][format!("{}", 300 + v)]["images"].get(0).cloned().ok_or("ComfyUI rendered no view")?;
            let bytes = comfy_view(c, &img)?;
            transcode::decode_rgb(&bytes).map_err(|e| format!("cannot read a view: {e}"))
        })
        .collect()
}

/// The AI's picture turned into frames for the fan, and kept under `key`.
fn finish(entity: &str, key: &str, bytes: &[u8], kind: Kind) -> Result<(Vec<RgbImage>, u32), String> {
    let mut img = transcode::decode_rgb(bytes).map_err(|e| format!("cannot read the AI's picture: {e}"))?;
    blacken_background(&mut img);
    log::info!("AI picture from {entity}: {}×{}", img.width(), img.height());
    let made = match kind {
        Kind::Still | Kind::Backdrop => (vec![transcode::fit(img, p56_service::MAX_SIDE)], 0),
        Kind::Spin => {
            // turned round the middle of the object itself, not of the picture
            let sq = square(img, 480);
            let centred = lit_middle(&sq).map_or(sq.clone(), |(mx, my)| cut(&sq, mx, my, 480, 480));
            (turntable(&centred, SPIN_FRAMES), SPIN_MS)
        }
        Kind::Anim => (sheet(&img, SHEET), SHEET_MS),
        Kind::Real3d => (vec![transcode::fit(img, p56_service::MAX_SIDE)], 0),
    };
    let mut kept = MADE.lock().unwrap();
    kept.retain(|(k, _, _)| *k != key);
    if kept.len() >= MADE_KEPT {
        kept.remove(0);
    }
    kept.push((key.to_string(), made.0.clone(), made.1));
    Ok(made)
}

/// The picture behind the AI's (signed) URL. Home Assistant serves it outside its REST API, so
/// it is fetched from Home Assistant itself: the add-on runs on the host network.
fn fetch_picture(base: &str, token: &str, path: &str) -> Result<Vec<u8>, String> {
    if path.starts_with("http://") {
        return crate::http_get(path, Some(token), "image/*").ok_or_else(|| format!("cannot fetch {path}"));
    }
    if !path.starts_with('/') {
        return Err(format!("unexpected picture address: {path}"));
    }
    let mut roots: Vec<String> = std::env::var("P56_HA_WEB").into_iter().collect();
    // P56_HA_URL in tests points at the fake Home Assistant itself
    if std::env::var("P56_HA_URL").is_ok() {
        roots.push(base.to_string());
    }
    roots.extend(["http://homeassistant:8123".to_string(), "http://127.0.0.1:8123".to_string()]);
    for root in &roots {
        if let Some(b) = crate::http_get(&format!("{}{path}", root.trim_end_matches('/')), None, "image/*") {
            return Ok(b);
        }
    }
    Err("the AI made a picture, but it could not be fetched from Home Assistant (port 8123, plain http)".into())
}

/// The middle square of a picture, `side` pixels wide.
fn square(img: RgbImage, side: u32) -> RgbImage {
    let s = img.width().min(img.height());
    let sq = image::imageops::crop_imm(&img, (img.width() - s) / 2, (img.height() - s) / 2, s, s).to_image();
    image::imageops::resize(&sq, side, side, image::imageops::FilterType::Triangle)
}

/// `n` frames of the object turning once round its own upright axis, as on a turntable: it
/// narrows to its edge, shows its (mirrored) back and widens again, a little darker while turned
/// sideways. The picture itself never tilts, so the fan does not seem to twist.
fn turntable(img: &RgbImage, n: u32) -> Vec<RgbImage> {
    let (w, h) = img.dimensions();
    let cx = (w as f32 - 1.0) / 2.0;
    (0..n)
        .map(|i| {
            let c = (std::f32::consts::TAU * i as f32 / n as f32).cos();
            let (width, shade) = (c.abs().max(0.03), 0.55 + 0.45 * c.abs());
            RgbImage::from_fn(w, h, |x, y| {
                // where this pixel comes from in the front view (mirrored when seen from behind)
                let sx = (x as f32 - cx) / width * c.signum() + cx;
                if sx < 0.0 || sx > w as f32 - 1.0 {
                    return image::Rgb([0, 0, 0]);
                }
                let (x0, f) = (sx.floor() as u32, sx.fract());
                let (a, b) = (img.get_pixel(x0, y).0, img.get_pixel((x0 + 1).min(w - 1), y).0);
                image::Rgb(std::array::from_fn(|k| ((a[k] as f32 * (1.0 - f) + b[k] as f32 * f) * shade).round() as u8))
            })
        })
        .collect()
}

/// A light background that reaches the edge (a white margin round the picture, or a white backdrop
/// the AI drew anyway) becomes black: the fan shows black as nothing and white as a lit disc.
fn blacken_background(img: &mut RgbImage) {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let mut todo: Vec<(u32, u32)> = (0..w).flat_map(|x| [(x, 0), (x, h - 1)]).chain((0..h).flat_map(|y| [(0, y), (w - 1, y)])).collect();
    while let Some((x, y)) = todo.pop() {
        if !img.get_pixel(x, y).0.iter().all(|&c| c >= LIGHT) {
            continue;
        }
        img.put_pixel(x, y, image::Rgb([0, 0, 0]));
        todo.extend([(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)].into_iter().filter(|&(x, y)| x < w && y < h));
    }
}

/// Every channel at least this bright counts as white background.
const LIGHT: u8 = 190;
/// A channel above this is a lit pixel.
const LIT: u8 = 48;

/// The frames of an `n`×`n` sprite sheet, in reading order. The AI does not always draw the grid
/// edge to edge, so the cells are found from the dark gaps between them; every frame has the same
/// size and the subject in its middle, so it stays put on the fan even when the AI let it drift.
fn sheet(img: &RgbImage, n: u32) -> Vec<RgbImage> {
    let lit = |p: &image::Rgb<u8>| p.0.iter().any(|&c| c > LIT);
    let mut cols = vec![0u32; img.width() as usize];
    let mut rows = vec![0u32; img.height() as usize];
    for (x, y, p) in img.enumerate_pixels() {
        if lit(p) {
            cols[x as usize] += 1;
            rows[y as usize] += 1;
        }
    }
    let (xs, w) = cell_centres(&cols, n);
    let (ys, h) = cell_centres(&rows, n);
    ys.iter()
        .flat_map(|&cy| xs.iter().map(move |&cx| (cx, cy)))
        .map(|(cx, cy)| {
            let (cx, cy) = (cx as i64, cy as i64);
            let frame = cut(img, cx, cy, w, h);
            match lit_middle(&frame) {
                Some((mx, my)) => cut(img, cx + mx - w as i64 / 2, cy + my - h as i64 / 2, w, h),
                None => frame,
            }
        })
        .collect()
}

/// A `w`×`h` piece of the picture around (cx, cy); outside the picture is black.
fn cut(img: &RgbImage, cx: i64, cy: i64, w: u32, h: u32) -> RgbImage {
    let mut frame = RgbImage::new(w, h);
    image::imageops::overlay(&mut frame, img, w as i64 / 2 - cx, h as i64 / 2 - cy);
    frame
}

/// The middle of the lit part of a picture (of its bounding box), if anything is lit.
fn lit_middle(img: &RgbImage) -> Option<(i64, i64)> {
    let lit: Vec<(u32, u32)> = img.enumerate_pixels().filter(|(_, _, p)| p.0.iter().any(|&c| c > LIT)).map(|(x, y, _)| (x, y)).collect();
    let (x0, x1) = (lit.iter().map(|p| p.0).min()?, lit.iter().map(|p| p.0).max()?);
    let (y0, y1) = (lit.iter().map(|p| p.1).min()?, lit.iter().map(|p| p.1).max()?);
    Some(((x0 + x1).div_ceil(2) as i64, (y0 + y1).div_ceil(2) as i64))
}

/// The centres of `n` cells along one axis and their size, from the lit pixels per column (or
/// row): the borders between cells are the darkest places near where they are expected.
fn cell_centres(profile: &[u32], n: u32) -> (Vec<u32>, u32) {
    let len = profile.len() as u32;
    let first = profile.iter().position(|&c| c > 0).unwrap_or(0) as u32;
    let last = profile.iter().rposition(|&c| c > 0).map_or(len, |i| i as u32 + 1);
    // nearly nothing lit: the whole picture
    let (start, end) = if last > first + 4 * n { (first, last) } else { (0, len) };
    let span = end - start;
    let win = span / (4 * n);
    let peak = profile.iter().copied().max().unwrap_or(0);
    let borders: Vec<u32> = (1..n)
        .map(|k| {
            let expect = start + span * k / n;
            let best = (expect - win..=expect + win).min_by_key(|&i| (profile[i as usize], i.abs_diff(expect))).unwrap_or(expect);
            // a sheet without dark gaps (e.g. falling rain) is simply cut evenly
            if profile[best as usize] > peak / 10 {
                return expect;
            }
            // else the middle of that dark gap (a few stray pixels still count as dark)
            let dark = |i: u32| profile[i as usize] <= profile[best as usize] + 2;
            let (mut a, mut b) = (best, best);
            while a > start && dark(a - 1) {
                a -= 1;
            }
            while b + 1 < end && dark(b + 1) {
                b += 1;
            }
            (a + b) / 2
        })
        .collect();
    // the size of a cell from the inner borders (the outer ones are where the drawing starts)
    let size = match borders.len() {
        0 => span,
        1 => span / 2,
        k => (borders[k - 1] - borders[0]) / (k as u32 - 1),
    }
    .max(1);
    let mut centres: Vec<u32> = borders.iter().map(|&b| (b + size / 2).min(len)).collect();
    centres.insert(0, borders.first().map_or(start + size / 2, |&b| b.saturating_sub(size / 2)));
    (centres, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_instructions() {
        assert_eq!(Kind::parse("").unwrap(), Kind::Still);
        assert_eq!(Kind::parse("anim").unwrap(), Kind::Anim);
        assert!(Kind::parse("video").is_err());
        let i = instructions("  a red fox ", Kind::Anim, None);
        assert!(i.contains("4 by 4") && i.ends_with("Subject: a red fox"), "{i}");
        assert!(instructions("fox", Kind::Spin, None).contains("from the front"));
        // no style of our own: only what the fan needs
        let plain = instructions("fox", Kind::Still, None);
        assert!(plain.contains("black") && !plain.contains("neon") && !plain.contains("Style"), "{plain}");
        assert!(instructions("fox", Kind::Still, Some("photo")).ends_with("Style: a realistic photograph"));
        assert!(instructions("fox", Kind::Still, Some(" Van Gogh ")).ends_with("Style: Van Gogh"));
        assert!(!instructions("fox", Kind::Still, Some("free")).contains("Style"));
        // a background behind text (notifications, prices): a whole simple picture about the theme,
        // without writing (and without the theme in quotes, which Qwen Image writes out)
        let b = instructions("cheap power", Kind::Backdrop, None);
        assert!(b.contains("little fine detail") && b.contains("situation: cheap power.") && b.contains("no letters"), "{b}");
        assert!(!b.contains("\"cheap power\""), "{b}");
    }

    #[test]
    fn an_object_turns_round_its_own_axis() {
        let img = RgbImage::from_fn(40, 40, |x, _| image::Rgb([if x < 20 { 255 } else { 0 }, 0, 0]));
        let frames = turntable(&img, 4);
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0], img, "the first frame is the front view");
        // a quarter turn: seen from the side, only a thin strip in the middle is left
        assert!((0..40).all(|y| frames[1].get_pixel(2, y).0[0] == 0 && frames[1].get_pixel(37, y).0[0] == 0));
        // half a turn: the back, mirrored (red right), and nothing tilted (every row the same)
        assert!(frames[2].get_pixel(35, 5).0[0] > 200 && frames[2].get_pixel(4, 5).0[0] == 0);
        assert!((0..40).all(|y| frames[2].get_pixel(35, y) == frames[2].get_pixel(35, 0)));
    }

    /// What Gemini drew once: the grid on a black square with a white margin round it, the cells
    /// not edge to edge. Every frame must be the subject in its own cell, at the same place, and
    /// no white may be left.
    #[test]
    fn a_sheet_with_a_white_margin_is_cut_at_its_cells() {
        let mut img = RgbImage::from_pixel(400, 400, image::Rgb([255, 255, 255]));
        // black square 30..370, cells of 85 px with a 20×40 red block in the middle of each,
        // drifting up to 9 px right and 6 px down (the AI letting it wander)
        for y in 30..370 {
            for x in 30..370 {
                img.put_pixel(x, y, image::Rgb([0, 0, 0]));
            }
        }
        for i in 0..16u32 {
            let (cx, cy) = (30 + (i % 4) * 85 + 42 + i % 10, 30 + (i / 4) * 85 + 42 + i % 7);
            for y in cy - 20..cy + 20 {
                for x in cx - 10..cx + 10 {
                    img.put_pixel(x, y, image::Rgb([230, 30, 30]));
                }
            }
        }
        blacken_background(&mut img);
        let frames = sheet(&img, 4);
        assert_eq!(frames.len(), 16);
        let size = frames[0].dimensions();
        for (i, f) in frames.iter().enumerate() {
            assert_eq!(f.dimensions(), size, "frame {i}");
            assert!(!f.pixels().any(|p| p.0.iter().all(|&c| c >= LIGHT)), "white in frame {i}");
            // the block is whole and in the middle of the frame, whatever its drift
            let red: Vec<(u32, u32)> = f.enumerate_pixels().filter(|(_, _, p)| p.0[0] > 200).map(|(x, y, _)| (x, y)).collect();
            assert_eq!(red.len(), 800, "frame {i}: the block is cut");
            let mx = red.iter().map(|p| p.0).sum::<u32>() / 800;
            let my = red.iter().map(|p| p.1).sum::<u32>() / 800;
            assert!(mx.abs_diff(size.0 / 2) <= 1 && my.abs_diff(size.1 / 2) <= 1, "frame {i}: block at {mx},{my} in {size:?}");
        }
        // a sheet without gaps (e.g. falling rain) is cut evenly
        let full = RgbImage::from_fn(400, 400, |x, y| image::Rgb([((y / 100 * 4 + x / 100) * 15 + 10) as u8, 200, 0]));
        let frames = sheet(&full, 4);
        assert_eq!(frames[0].dimensions(), (100, 100));
        for (i, f) in frames.iter().enumerate() {
            assert!(f.pixels().all(|p| p.0[0] == (i * 15 + 10) as u8), "frame {i} is not exactly its cell");
        }
        // a white subject inside the black square is kept
        let mut solo = RgbImage::from_pixel(50, 50, image::Rgb([0, 0, 0]));
        solo.put_pixel(25, 25, image::Rgb([255, 255, 255]));
        blacken_background(&mut solo);
        assert_eq!(solo.get_pixel(25, 25).0, [255, 255, 255]);
    }
}
