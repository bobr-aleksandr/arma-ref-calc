#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;
use std::mem;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Direct3D::*,
            Direct3D11::*,
            DirectComposition::*,
            DirectWrite::*,
            Dxgi::{Common::*, *},
            Gdi::*,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
};

fn loword(l: u32) -> u16 { (l & 0xFFFF) as u16 }
fn hiword(l: u32) -> u16 { ((l >> 16) & 0xFFFF) as u16 }

#[derive(Clone, Copy)] 
struct SafeHWND(HWND);
unsafe impl Send for SafeHWND {}
unsafe impl Sync for SafeHWND {}

const WM_UPDATE_OVERLAY: u32 = WM_USER + 1;
const WM_UPDATE_SETTINGS_INPUTS: u32 = WM_USER + 2;

const ID_HOTKEY_TOGGLE_SETTINGS: i32 = 1;
const ID_HOTKEY_TOGGLE_OVERLAY: i32 = 2;
const ID_HOTKEY_RELOAD: i32 = 3;
const ID_HOTKEY_SET_KV: i32 = 4;       
const ID_HOTKEY_SET_CAM: i32 = 5;      
const ID_HOTKEY_SET_TARGET_FROM_COORDS: i32 = 6;
const ID_HOTKEY_SET_TARGET: i32 = 7;   
const ID_HOTKEY_CLEAR: i32 = 8;

const ID_BTN_CALCULATE: u16 = 100;
const ID_BTN_TOGGLE_OVERLAY: u16 = 101;
const ID_COMBO_GUN: u16 = 102;
const ID_BTN_QUIT: u16 = 104; 

const ID_EDIT_SELF_X: u16 = 10;
const ID_EDIT_SELF_Y: u16 = 11;
const ID_EDIT_SELF_Z: u16 = 12;
const ID_EDIT_TARG_X: u16 = 13;
const ID_EDIT_TARG_Y: u16 = 14;
const ID_EDIT_TARG_Z: u16 = 15;
const ID_EDIT_MAP_KV: u16 = 16;
const ID_EDIT_MAP_SCALE: u16 = 17;

const ID_EDIT_WIND_SPEED: u16 = 18;
const ID_EDIT_WIND_DIR: u16 = 19;
const ID_EDIT_OBJ_HEIGHT: u16 = 20; 

// ── PERSPECTIVE PROJECTION ──

fn angles_to_pixel(
    yaw: f64,
    pitch: f64,
    screen_width: f64,
    screen_height: f64,
    fov_degrees: f64,
) -> (f64, f64) {
    let fov_rad = fov_degrees.to_radians();
    let half_fov_tan = (fov_rad / 2.0).tan();
    let projection_distance = (screen_height / 2.0) / half_fov_tan;
    
    let centered_x = projection_distance * yaw.tan();
    let centered_y = projection_distance * pitch.tan() / yaw.cos();
    
    let x = centered_x + (screen_width / 2.0);
    let y = (screen_height / 2.0) - centered_y;
    (x, y)
}

// ── BALLISTICS LOGIC ──

#[derive(Clone, Copy, PartialEq)]
enum GunType {
    Gun122,
    Gun122v,
    Gun125,
    GunAGS,
    Gun82he1,
    Gun82he2,
    Gun82he3,
    Gun82he4,
    Gunm7771,
    Gunm7772,
    Gunm7773,
    Gunm7774,
    Gunm7775,
}

impl GunType {
    // (v0, mass, Cx, max_dist, high_arc, adj, mouse_mult, fov, lat_cx_mult, az_correction_flag, mils_per_circle)
    fn params(&self) -> (f64, f64, f64, f64, bool, f64, f64, f64, f64, bool, f64) {
        match self {
            // NATO 6400
            GunType::Gun122 => (240.0, 15.0, 0.0008, 2900.0, false, 0.246, 10.0, 74.0, 10.0, false, 6400.0),
            GunType::Gun122v => (240.0, 15.0, 0.0008, 2900.0, true, 0.246, 10.0, 74.0, 10.0, false, 6400.0),
            
            // Soviet 6000
            GunType::Gun125 => (915.0, 23.0, 0.016, 2900.0, false, 0.0, 6.03, 14.15, 10.0, true, 6000.0),
            GunType::GunAGS => (310.000, 0.026, 0.0000001, 2900.0, false, 0.0, 6.03, 14.15, 10.0, true, 6000.0),
            GunType::Gun82he1 => (76.000*1.321, 3.10000, 0.0006150, 2900.0, true, 0.0, 10.0, 74.0, 10.0, false, 6000.0),
            GunType::Gun82he2 => (76.000*1.736, 3.10000, 0.0006150, 2900.0, true, 0.0, 10.0, 74.0, 10.0, false, 6000.0),
            GunType::Gun82he3 => (76.000*2.087, 3.10000, 0.0006150, 2900.0, true, 0.0, 10.0, 74.0, 10.0, false, 6000.0),
            GunType::Gun82he4 => (76.000*2.455, 3.10000, 0.0006150, 2900.0, true, 0.0, 10.0, 74.0, 10.0, false, 6000.0),
            GunType::Gunm7771 => (100.000*1.400, 43.00000, 0.0097000, 2900.0, false, 0.0, 10.0, 74.0, 10.0, false, 6400.0),
            GunType::Gunm7772=> (100.000*2.045, 43.00000, 0.0097000, 2900.0, false, 0.0, 10.0, 74.0, 10.0, false, 6400.0),
            GunType::Gunm7773=> (100.000*2.790, 43.00000, 0.0097000, 2900.0, false, 0.0, 10.0, 74.0, 10.0, false, 6400.0),
            GunType::Gunm7774 => (100.000*3.535, 43.00000, 0.0097000, 2900.0, false, 0.0, 10.0, 74.0, 10.0, false, 6400.0),
            GunType::Gunm7775 => (100.000*4.280, 43.00000, 0.0097000, 2900.0, false, 0.0, 10.0, 74.0, 10.0, false, 6400.0),
            
        }   
    }
    
    fn name(&self) -> &'static str {
        match self {
            GunType::Gun122 => "122",
            GunType::Gun122v => "122v",
            GunType::Gun125 => "125",
            GunType::GunAGS => "AGS",
            GunType::Gun82he1 => "82he1",
            GunType::Gun82he2 => "82he2",
            GunType::Gun82he3 => "82he3",
            GunType::Gun82he4 => "82he4",
            GunType::Gunm7771 => "m777he1",
            GunType::Gunm7772 => "m777he2 ",
            GunType::Gunm7773 => "m777he3",
            GunType::Gunm7774 => "m777he4",
            GunType::Gunm7775 => "m777he5",

        }
    }
}

#[derive(Clone, Default)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Default)]
struct CalculationResult {
    azimuth: f64,
    distance: f64,
    elevation: f64,
    time_of_flight: f64,
    lateral_drift: f64, 
    valid: bool,
}

fn calculate_flow_displacement(
    flow_speed: f64,
    time: f64,
    cx: f64,
    mass: f64
) -> f64 {
    let dt = 0.001; 
    let mut x = 0.0;
    let mut v = 0.0; 
    
    let steps = (time / dt).ceil() as i32;

    for _ in 0..steps {
        let v_rel = v - flow_speed;
        let speed_rel = v_rel.abs();
        let a = if speed_rel > 0.0 {
            -cx * speed_rel.powi(2) * v_rel.signum() / mass
        } else {
            0.0
        };
        v += a * dt;
        x += v * dt;
    }
    x
}

fn calculate_trajectory(
    theta_deg: f64, 
    v0: f64, 
    mass: f64, 
    cx: f64, 
    dy: f64, 
    g: f64, 
    potok: f64 
) -> (f64, f64) {
    let dt = 0.0166;
    let theta = theta_deg.to_radians();
    let mut vx = v0 * theta.cos();
    let mut vy = v0 * theta.sin();
    let mut x = 0.0;
    let mut y = dy; 
    let mut t = 0.0;

    for _ in 0..1000000 {
        let v_rel_x = vx - potok;
        let v_rel_y = vy; 
        let speed_rel = (v_rel_x.powi(2) + v_rel_y.powi(2)).sqrt();
        let f_drag = cx * speed_rel.powi(2);
        
        let ax = if speed_rel > 0.0 { -f_drag * v_rel_x / (mass * speed_rel) } else { 0.0 };
        let ay = if speed_rel > 0.0 { -g - f_drag * v_rel_y / (mass * speed_rel) } else { -g };

        vx += ax * dt;
        vy += ay * dt;
        x += vx * dt;
        y += vy * dt;
        t += dt;

        if vy < 0.0 && y < 0.0 {
            break;
        }
    }
    (x, t)
}

fn find_optimal_angle(
    target_distance: f64, 
    v0: f64, 
    mass: f64, 
    cx: f64, 
    dy: f64, 
    high_arc: bool, 
    potok_speed:f64
) -> f64 {
    let g = 9.81;
    let precision = 0.00001;
    let max_iter = 100;
    let mut iterations = 0;

    let (mut low, mut high) = if high_arc {
        (45.0, 90.0)
    } else {
        (0.0, 45.0)
    };

    while (high - low) > precision && iterations < max_iter {
        let mid = (low + high) / 2.0;
        let current_dist = calculate_trajectory(mid, v0, mass, cx, dy, g, potok_speed).0;
            
        if high_arc {
            if current_dist > target_distance { low = mid; } else { high = mid; }
        } else {
            if current_dist < target_distance { low = mid; } else { high = mid; }
        }
        iterations += 1;
    }
    (low + high) / 2.0
}

unsafe fn sync_map_kv_from_settings(st: &mut SharedState) {
    if st.settings_hwnd.0 == HWND::default() { return; }
    if let Ok(h) = GetDlgItem(st.settings_hwnd.0, ID_EDIT_MAP_KV as i32) {
        let len = GetWindowTextLengthW(h) + 1;
        if len > 0 {
            let mut buf = vec![0u16; len as usize];
            let _ = GetWindowTextW(h, &mut buf);
            st.map_kv_str = String::from_utf16_lossy(&buf).trim().to_string();
        }
    }
}

fn recalc_target_from_pixels(st: &mut SharedState) {
    if st.t_pixel_pos == (0, 0) || st.kv_pixel_pos == (0, 0) || st.map_scale <= 0.0 {
        return;
    }
    let (mx, mz) = parse_map_kv_base(&st.map_kv_str);
    let dx_px = st.t_pixel_pos.0 - st.kv_pixel_pos.0;
    let dy_px = st.kv_pixel_pos.1 - st.t_pixel_pos.1;
    st.target_pos.x = mx + dx_px as f64 * st.map_scale;
    st.target_pos.z = mz + dy_px as f64 * st.map_scale;
    st.result = run_calculation(&st.self_pos, &st.target_pos, st.gun, st.wind_speed, st.wind_dir, st.obj_height);
}

fn parse_map_kv_base(map_kv: &str) -> (f64, f64) {
    let s = map_kv.trim();
    if s.len() >= 6 {
        if let (Ok(xp), Ok(zp)) = (s[..3].parse::<i32>(), s[3..6].parse::<i32>()) {
            return (xp as f64 * 100.0, zp as f64 * 100.0);
        }
    }
    (0.0, 0.0)
}

fn run_calculation(
    self_pos: &Vec3, 
    target_pos: &Vec3, 
    gun: GunType, 
    wind_speed: f64, 
    wind_dir: f64,
    obj_height: f64 
) -> CalculationResult {
    let dx = self_pos.x - target_pos.x;
    let dz = self_pos.z - target_pos.z;
    let distance = (dx.powi(2) + dz.powi(2)).sqrt();
    let dy = self_pos.y - target_pos.y ;

    let mut azimuth = dz.atan2(-dx).to_degrees() + 90.0;
    if azimuth < 0.0 { azimuth += 360.0; }

    let (v0, mass, cx, _max_range, high_arc, adj, _mouse_mult, _fov, lat_cx_mult, apply_el_correction, _mils) = gun.params();

    let d_napr_vetra = (azimuth-wind_dir).to_radians();
    let potok_speed_lobash = wind_speed*(-d_napr_vetra.cos());
    let potok_speed_bok = wind_speed*(-d_napr_vetra.sin());

    let raw_angle = find_optimal_angle(distance, v0, mass, cx, dy, high_arc, potok_speed_lobash);
    let (check_dist, time_of_flight) = calculate_trajectory(raw_angle, v0, mass, cx, dy, 9.81, potok_speed_lobash);

    let smeshenie = calculate_flow_displacement(potok_speed_bok, time_of_flight, cx * lat_cx_mult, mass);
    let d_angle = ((smeshenie/check_dist).atan()).to_degrees();
    azimuth = azimuth + d_angle;

    let mut final_elevation = raw_angle + adj;
    
    if apply_el_correction {
        if dy != 0.0 {
            let correction_rad = (-dy / distance).atan();
            final_elevation -= correction_rad.to_degrees();
        }
    }

    if distance > 0.0 && obj_height > 0.0 {
        let height_correction = (obj_height / distance).atan().to_degrees();
        final_elevation += height_correction;
    }

    CalculationResult {
        azimuth,
        distance,
        elevation: final_elevation,
        time_of_flight: time_of_flight, 
        lateral_drift: smeshenie, 
        valid: true,
    }
}

// ── STATE ──

#[derive(Clone)] 
struct SharedState {
    overlay_hwnd: SafeHWND,
    settings_hwnd: SafeHWND,
    visible: bool,
    
    self_pos: Vec3,
    target_pos: Vec3,
    gun: GunType,
    
    result: CalculationResult,
    
    reload_start_time: Option<Instant>,
    reload_duration: Duration,

    status_text: Option<String>,
    status_expires_at: Option<Instant>,

    timer_label: Option<String>,

    show_coords_helpers: bool,
    kv_pixel_pos: (i32, i32),
    c_pixel_pos: (i32, i32),
    t_pixel_pos: (i32, i32),
    map_kv_str: String,
    map_scale: f64,

    wind_speed: f64,
    wind_dir: f64,
    obj_height: f64,
    
    auto_elev_fired: bool,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            overlay_hwnd: SafeHWND(HWND::default()),
            settings_hwnd: SafeHWND(HWND::default()),
            visible: true,
            
            self_pos: Vec3 { x:4757.0, y: 26.0, z: 12056.0 },
            target_pos: Vec3 { x: 4853.0, y: 116.0, z: 8050.0 },
            gun: GunType::Gun125,
            
            result: Default::default(),
            
            reload_start_time: None,
            reload_duration: Duration::from_secs_f32(5.0),
            
            status_text: None,
            status_expires_at: None,
            
            timer_label: None,

            show_coords_helpers: true,
            kv_pixel_pos: (0, 0),
            c_pixel_pos: (0, 0),
            t_pixel_pos: (0, 0),
            map_kv_str: "000000".to_string(),
            map_scale: 0.05,

            wind_speed: 0.0,
            wind_dir: 0.0,
            obj_height: 0.0, 
            auto_elev_fired: false,
        }
    }
}

// ── D2D RESOURCES ──

struct D2DResources {
    _d2d_factory: ID2D1Factory1,
    _dwrite_factory: IDWriteFactory,
    text_format: IDWriteTextFormat,
    text_format_large: IDWriteTextFormat,
    dcomp_device: IDCompositionDevice,
    _dcomp_target: IDCompositionTarget,
    _dcomp_visual: IDCompositionVisual,
    swap_chain: IDXGISwapChain1,
    d2d_context: ID2D1DeviceContext,
    
    brush_text: ID2D1SolidColorBrush,
    _brush_bg: ID2D1SolidColorBrush,
    brush_good: ID2D1SolidColorBrush, 
    brush_bad: ID2D1SolidColorBrush,  
    brush_bar_bg: ID2D1SolidColorBrush,
    brush_bar_fg: ID2D1SolidColorBrush,
    brush_kv: ID2D1SolidColorBrush,
    brush_cam: ID2D1SolidColorBrush,
    brush_tar: ID2D1SolidColorBrush,
    _brush_self_proj: ID2D1SolidColorBrush,

    screen_w: f32,
    screen_h: f32,
}

// ── MAIN ──

fn main() -> Result<()> {
    let state = Arc::new(Mutex::new(SharedState::default()));
    unsafe { run_win32_app(state) }
}

// ── WIN32 APP ──

unsafe fn run_win32_app(state: Arc<Mutex<SharedState>>) -> Result<()> {
    let instance = GetModuleHandleW(None)?;
    let h_inst = HINSTANCE(instance.0);

    let overlay_class = w!("OverlayClass");
    let _ = RegisterClassExW(&WNDCLASSEXW {
        cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(overlay_wnd_proc),
        hInstance: h_inst,
        lpszClassName: overlay_class,
        ..Default::default()
    });

    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);

    let state_for_overlay = state.clone();
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        overlay_class,
        w!("Overlay"),
        WS_POPUP,
        0, 0, screen_w, screen_h,
        HWND::default(),
        HMENU::default(),
        h_inst,
        Some(Arc::into_raw(state_for_overlay) as *const _),
    )?;

    state.lock().unwrap().overlay_hwnd = SafeHWND(hwnd);
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = UpdateWindow(hwnd);

    let _ = RegisterHotKey(hwnd, ID_HOTKEY_TOGGLE_SETTINGS, MOD_CONTROL, 0x31); 
    let _ = RegisterHotKey(hwnd, ID_HOTKEY_RELOAD, MOD_CONTROL, 0x32); 
    let _ = RegisterHotKey(hwnd, ID_HOTKEY_SET_KV, MOD_CONTROL, 0x33); 
    let _ = RegisterHotKey(hwnd, ID_HOTKEY_SET_CAM, MOD_CONTROL, 0x34); 
    let _ = RegisterHotKey(hwnd, ID_HOTKEY_SET_TARGET_FROM_COORDS, MOD_CONTROL, 0x35); 
    let _ = RegisterHotKey(hwnd, ID_HOTKEY_SET_TARGET, MOD_CONTROL, 0x36); 
    
    let _ = RegisterHotKey(hwnd, ID_HOTKEY_TOGGLE_OVERLAY, HOT_KEY_MODIFIERS(0), 0xBB); 
    let _ = RegisterHotKey(hwnd, ID_HOTKEY_CLEAR, HOT_KEY_MODIFIERS(0), 0xBD); 

    let settings_class = w!("SettingsClass");
    let _ = RegisterClassExW(&WNDCLASSEXW {
        cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(settings_wnd_proc),
        hInstance: h_inst,
        lpszClassName: settings_class,
        style: CS_HREDRAW | CS_VREDRAW,
        hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut c_void),
        ..Default::default()
    });

    let state_for_settings = state.clone();
    let settings_wnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_WINDOWEDGE,
        settings_class,
        w!("Artillery Calc Rust"),
        WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX & !WS_THICKFRAME & !WS_SYSMENU,
        CW_USEDEFAULT, CW_USEDEFAULT,
        250, 850,
        HWND::default(),
        HMENU::default(),
        h_inst,
        Some(Arc::into_raw(state_for_settings) as *const _),
    )?;

    state.lock().unwrap().settings_hwnd = SafeHWND(settings_wnd);
    
    let _ = ShowWindow(settings_wnd, SW_SHOW);
    let _ = UpdateWindow(settings_wnd);

    let _ = PostMessageW(hwnd, WM_UPDATE_OVERLAY, WPARAM(0), LPARAM(0));

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    Ok(())
}

// ── GRAPHICS & RENDERING ──

struct WindowData {
    resources: D2DResources,
    state: Arc<Mutex<SharedState>>,
}

unsafe fn init_graphics(hwnd: HWND, state: Arc<Mutex<SharedState>>) -> Result<()> {
    let d3d_device = create_d3d_device()?;
    let dxgi_device: IDXGIDevice = d3d_device.cast()?;
    let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
    let d2d_device = d2d_factory.CreateDevice(&dxgi_device)?;
    let d2d_context = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

    let screen_w = GetSystemMetrics(SM_CXSCREEN) as f32;
    let screen_h = GetSystemMetrics(SM_CYSCREEN) as f32;

    let factory: IDXGIFactory2 = {
        let adapter: IDXGIAdapter = dxgi_device.GetAdapter()?;
        adapter.GetParent()?
    };

    let swap_chain = factory.CreateSwapChainForComposition(
        &d3d_device,
        &DXGI_SWAP_CHAIN_DESC1 {
            Width: screen_w as u32,
            Height: screen_h as u32,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            ..Default::default()
        },
        None,
    )?;

    let dcomp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
    let dcomp_target = dcomp_device.CreateTargetForHwnd(hwnd, true)?;
    let dcomp_visual = dcomp_device.CreateVisual()?;
    dcomp_visual.SetContent(&swap_chain)?;
    dcomp_target.SetRoot(&dcomp_visual)?;
    dcomp_device.Commit()?;

    let dwrite_factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
    let text_format = dwrite_factory.CreateTextFormat(
        w!("Consolas"),
        None,
        DWRITE_FONT_WEIGHT_NORMAL,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        18.0,
        w!("en-us"),
    )?;
    let _ = text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
    let _ = text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);

    let text_format_large = dwrite_factory.CreateTextFormat(
        w!("Consolas"),
        None,
        DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        24.0,
        w!("en-us"),
    )?;
    let _ = text_format_large.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
    let _ = text_format_large.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

    let brush_text = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
    let brush_bg = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.6 }, None)?;
    let brush_good = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 1.0, b: 0.0, a: 1.0 }, None)?; 
    let brush_bad = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, None)?; 
    let brush_bar_bg = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.2, g: 0.2, b: 0.2, a: 1.0 }, None)?;
    let brush_bar_fg = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, None)?;
    
    let brush_kv = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 0.0, a: 1.0 }, None)?;
    let brush_cam = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }, None)?;
    let brush_tar = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 1.0, a: 1.0 }, None)?;
    let brush_self_proj = d2d_context.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 1.0, b: 0.0, a: 1.0 }, None)?;

    let resources = D2DResources {
        _d2d_factory: d2d_factory,
        _dwrite_factory: dwrite_factory,
        text_format,
        text_format_large,
        dcomp_device,
        _dcomp_target: dcomp_target,
        _dcomp_visual: dcomp_visual,
        swap_chain,
        d2d_context,
        brush_text,
        _brush_bg: brush_bg,
        brush_good,
        brush_bad,
        brush_bar_bg,
        brush_bar_fg,
        brush_kv,
        brush_cam,
        brush_tar,
        _brush_self_proj: brush_self_proj,
        screen_w,
        screen_h,
    };

    let data = WindowData { resources, state };
    let ptr = Box::into_raw(Box::new(data));
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
    Ok(())
}

unsafe fn create_d3d_device() -> Result<ID3D11Device> {
    let mut d3d_device = None;
    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        None,
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        Some(&[D3D_FEATURE_LEVEL_11_0]),
        D3D11_SDK_VERSION,
        Some(&mut d3d_device),
        None,
        None,
    )?;
    Ok(d3d_device.unwrap())
}

unsafe fn do_render(hwnd: HWND) {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if ptr == 0 { return; }
    let data = &mut *(ptr as *mut WindowData);

    let (snapshot, reload_pct, is_reloading, current_status_msg, current_timer_label, coords_state) = {
        let mut st = data.state.lock().unwrap();
        
        let msg_to_display = if let Some(expiry) = st.status_expires_at {
            if Instant::now() < expiry {
                st.status_text.clone()
            } else {
                st.status_text = None;
                st.status_expires_at = None;
                None
            }
        } else {
            st.status_text.clone()
        };

        let pct = if let Some(start) = st.reload_start_time {
            let elapsed = start.elapsed();
            if elapsed < st.reload_duration {
                1.0 - (elapsed.as_secs_f32() / st.reload_duration.as_secs_f32())
            } else {
                0.0
            }
        } else {
            0.0
        };

        let c_state = (
            st.show_coords_helpers,
            st.kv_pixel_pos,
            st.c_pixel_pos,
            st.t_pixel_pos,
        );
        
        ((*st).clone(), pct, st.reload_start_time.is_some(), msg_to_display, st.timer_label.clone(), c_state)
    };

    let surface: IDXGISurface = data.resources.swap_chain.GetBuffer(0).unwrap();
    let bmp_props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        ..Default::default()
    };

    let bitmap = data.resources.d2d_context.CreateBitmapFromDxgiSurface(&surface, Some(&bmp_props)).unwrap();
    data.resources.d2d_context.SetTarget(&bitmap);
    data.resources.d2d_context.BeginDraw();
    data.resources.d2d_context.Clear(None);
    
    if snapshot.visible {
        let screen_w = data.resources.screen_w;
        let screen_h = data.resources.screen_h;
        let centerx = screen_w / 2.0;
        let centery = screen_h / 2.0;
        
        let (_, _, _, _, _, _, _, fov, .., mils_per_circle) = snapshot.gun.params();
        
        let (cross_x, cross_y) = if snapshot.result.valid {
            let pitch_rad = -snapshot.result.elevation.to_radians();
            
            let yaw_offset_rad = if (snapshot.result.distance).abs() > 0.0 {
                -(snapshot.result.lateral_drift / snapshot.result.distance).atan()
            } else {
                0.0
            };
            
            angles_to_pixel(yaw_offset_rad, pitch_rad, screen_w as f64, screen_h as f64, fov)
        } else {
            (centerx as f64, centery as f64)
        };

        let cross_size = 10.0;
        let cx = cross_x as f32;
        let cy = cross_y as f32;
        data.resources.d2d_context.DrawLine(
            D2D_POINT_2F { x: cx - cross_size, y: centery },
            D2D_POINT_2F { x: cx + cross_size, y: centery },
            &data.resources.brush_text, 1.0, None
        );
        data.resources.d2d_context.DrawLine(
            D2D_POINT_2F { x: cx, y: centery - cross_size },
            D2D_POINT_2F { x: cx, y: centery + cross_size },
            &data.resources.brush_text, 1.0, None
        );
        data.resources.d2d_context.DrawLine(
            D2D_POINT_2F { x: cx - cross_size, y: cy },
            D2D_POINT_2F { x: cx + cross_size, y: cy },
            &data.resources.brush_text, 1.0, None
        );
        data.resources.d2d_context.DrawLine(
            D2D_POINT_2F { x: cx, y: cy - cross_size },
            D2D_POINT_2F { x: cx, y: cy + cross_size },
            &data.resources.brush_text, 1.0, None
        );

        if coords_state.0 {
            let (kv, cam, tar) = (coords_state.1, coords_state.2, coords_state.3);
            
            if kv != (0,0) {
                let kx = kv.0 as f32; let ky = kv.1 as f32;
                data.resources.d2d_context.DrawLine(D2D_POINT_2F{x: kx+2.0, y: ky+1.0}, D2D_POINT_2F{x: kx+50.0, y: ky+1.0}, &data.resources.brush_kv, 2.0, None);
                data.resources.d2d_context.DrawLine(D2D_POINT_2F{x: kx+2.0, y: ky+1.0}, D2D_POINT_2F{x: kx+2.0, y: ky-47.0}, &data.resources.brush_kv, 2.0, None);
            }

            if cam != (0,0) {
                let cx_px = cam.0 as f32; let cy_px = cam.1 as f32;
                data.resources.d2d_context.DrawLine(D2D_POINT_2F{x: cx_px+2.0, y: cy_px+25.0}, D2D_POINT_2F{x: cx_px+2.0, y: cy_px-25.0}, &data.resources.brush_cam, 2.0, None);
            }

            if tar != (0,0) {
                let tx = tar.0 as f32; let ty = tar.1 as f32;
                data.resources.d2d_context.DrawLine(D2D_POINT_2F{x: tx-25.0, y: ty+2.0}, D2D_POINT_2F{x: tx+28.0, y: ty+2.0}, &data.resources.brush_tar, 2.0, None);
                data.resources.d2d_context.DrawLine(D2D_POINT_2F{x: tx+2.0, y: ty-25.0}, D2D_POINT_2F{x: tx+2.0, y: ty+28.0}, &data.resources.brush_tar, 2.0, None);
                
                if kv != (0,0) {
                    data.resources.d2d_context.DrawLine(D2D_POINT_2F{x: kv.0 as f32, y: kv.1 as f32}, D2D_POINT_2F{x: tx, y: ty}, &data.resources.brush_tar, 1.0, None);
                }
            }
        }

        let box_x = 50.0;
        let box_y = 50.0;
        let box_w = 300.0;
        let box_h = 160.0;
        
        let bg_rect = D2D_RECT_F { left: box_x, top: box_y, right: box_x + box_w, bottom: box_y + box_h };
        data.resources.d2d_context.DrawRectangle(&bg_rect, &data.resources.brush_text, 1.0, None);

        let mut cursor_y = box_y + 10.0;
        let line_height = 25.0;

        let draw_text = |text: &str, color: &ID2D1SolidColorBrush, y: f32| {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let rect = D2D_RECT_F { left: box_x + 10.0, top: y, right: box_x + box_w - 10.0, bottom: y + line_height };
            data.resources.d2d_context.DrawText(&wide, &data.resources.text_format, &rect, color, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        };

        let title_brush = if snapshot.result.valid { &data.resources.brush_good } else { &data.resources.brush_bad };
        
        draw_text(&format!("GUN: {}", snapshot.gun.name()), &data.resources.brush_text, cursor_y);
        cursor_y += line_height;

        if snapshot.result.valid {
            draw_text(&format!("Azimuth : {:.3}", snapshot.result.azimuth), title_brush, cursor_y); cursor_y += line_height;
            draw_text(&format!("Elev    : {:.3}", snapshot.result.elevation), title_brush, cursor_y); cursor_y += line_height;
            draw_text(&format!("Dist    : {:.1} m", snapshot.result.distance), title_brush, cursor_y); cursor_y += line_height;
            draw_text(&format!("Time    : {:.2} s", snapshot.result.time_of_flight), title_brush, cursor_y);

            let mils_factor = mils_per_circle / 360.0;
            let az_mils = (snapshot.result.azimuth * mils_factor).round() as i32;
            let el_mils = (snapshot.result.elevation * mils_factor).round() as i32;

            let elev_x = screen_w * (323.0 / 1920.0);
            let elev_y = screen_h * (499.0 / 1080.0);
            
            let napr_x = screen_w * (932.0 / 1920.0);
            let napr_y = screen_h * (967.0 / 1080.0);

            let draw_hud_text = |text: &str, x: f32, y: f32| {
                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let rect = D2D_RECT_F { 
                    left: x, 
                    top: y - 20.0, 
                    right: x + 200.0, 
                    bottom: y + 20.0 
                };
                data.resources.d2d_context.DrawText(
                    &wide, 
                    &data.resources.text_format_large, 
                    &rect, 
                    &data.resources.brush_bad,
                    D2D1_DRAW_TEXT_OPTIONS_NONE, 
                    DWRITE_MEASURING_MODE_NATURAL
                );
            };

            draw_hud_text(&format!("{}", el_mils), elev_x, elev_y);
            draw_hud_text(&format!("{}", az_mils), napr_x, napr_y);

        } else {
            draw_text(&format!("Azimuth : {:.3}", snapshot.result.azimuth), &data.resources.brush_bad, cursor_y); 
            cursor_y += line_height;
            draw_text(&format!("Dist    : {:.1} m", snapshot.result.distance), &data.resources.brush_bad, cursor_y); 
            cursor_y += line_height;
            
            draw_text("OUT OF RANGE / IMPOSSIBLE", &data.resources.brush_bad, cursor_y);
        }

        let bar_y = screen_h - 250.0; 

        if let Some(text) = current_status_msg {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let text_rect = D2D_RECT_F { 
                left: 0.0, 
                top: bar_y - 50.0, 
                right: screen_w, 
                bottom: bar_y + 50.0 
            };
            let fmt = data.resources.text_format_large.clone();
            let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            
            data.resources.d2d_context.DrawText(
                &wide, 
                &fmt, 
                &text_rect, 
                &data.resources.brush_bad, 
                D2D1_DRAW_TEXT_OPTIONS_NONE, 
                DWRITE_MEASURING_MODE_NATURAL
            );
        } else if is_reloading && reload_pct > 0.0 {
            let bar_w = 300.0;
            let bar_h = 20.0;
            let bar_x = (screen_w - bar_w) / 2.0;

            let bg_bar_rect = D2D_RECT_F { left: bar_x, top: bar_y, right: bar_x + bar_w, bottom: bar_y + bar_h };
            data.resources.d2d_context.FillRectangle(&bg_bar_rect, &data.resources.brush_bar_bg);
            data.resources.d2d_context.DrawRectangle(&bg_bar_rect, &data.resources.brush_text, 1.0, None);

            let current_w = bar_w * reload_pct;
            let fg_bar_rect = D2D_RECT_F { left: bar_x, top: bar_y, right: bar_x + current_w, bottom: bar_y + bar_h };
            data.resources.d2d_context.FillRectangle(&fg_bar_rect, &data.resources.brush_bar_fg);

            let label_text = current_timer_label.unwrap_or_else(|| "RELOADING...".to_string());
            let wide: Vec<u16> = label_text.encode_utf16().chain(std::iter::once(0)).collect();
            let text_rect = D2D_RECT_F { left: bar_x, top: bar_y - 25.0, right: bar_x + bar_w, bottom: bar_y };
            data.resources.d2d_context.DrawText(&wide, &data.resources.text_format, &text_rect, &data.resources.brush_bad, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    data.resources.d2d_context.EndDraw(None, None);
    data.resources.swap_chain.Present(0, DXGI_PRESENT(0)).ok();
    data.resources.dcomp_device.Commit();
}

// ── WINDOW PROCEDURES ──

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let create_struct = &*(lparam.0 as *const CREATESTRUCTW);
            let state_ptr = create_struct.lpCreateParams;
            let state = Arc::from_raw(state_ptr as *const Mutex<SharedState>);
            let state_clone = state.clone();
            mem::forget(state);

            let _ = init_graphics(hwnd, state_clone);
            let _ = SetTimer(hwnd, 1, 16, None);
            LRESULT(0)
        }
        WM_TIMER => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if state_ptr != 0 {
                let data = &*(state_ptr as *const WindowData);
                
                let q_pressed = GetAsyncKeyState(0x51) & 0x8000u16 as i16 != 0; 
                let e_pressed = GetAsyncKeyState(0x45) & 0x8000u16 as i16 != 0; 
                
                if q_pressed && e_pressed {
                    let mut st = data.state.lock().unwrap();
                    if !st.auto_elev_fired && st.result.valid {
                        st.auto_elev_fired = true;
                        
                        let elevation = st.result.elevation;
                        let gun = st.gun;
                        
                        drop(st); 
                        
                        let (_, _, _, _, _, _, elev_mult, .., mils_per_circle) = gun.params();
                        
                        let mils_factor = mils_per_circle / 360.0;
                        let el_mils = (elevation * mils_factor).round() as i32;
                        
                        let kolvo = (el_mils.abs() as f64 * elev_mult).round() as u32;
                        let znak = if el_mils >= 0 { 1 } else { -1 };
                        
                        for _ in 0..kolvo {
                             let mouse_input = MOUSEINPUT {
                                dx: 0,
                                dy: -1 * znak, 
                                mouseData: 0,
                                dwFlags: MOUSEEVENTF_MOVE,
                                time: 0,
                                dwExtraInfo: 0,
                            };
                            let input = INPUT {
                                r#type: INPUT_MOUSE,
                                Anonymous: INPUT_0 { mi: mouse_input },
                            };
                            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
                            std::thread::sleep(Duration::from_millis(2));
                        }
                    }
                } else {
                    let mut st = data.state.lock().unwrap();
                    if st.auto_elev_fired {
                        st.auto_elev_fired = false;
                    }
                }

                let mut st = data.state.lock().unwrap();
                let mut needs_render = false;

                if let Some(start) = st.reload_start_time {
                    let elapsed = start.elapsed();
                    if elapsed < st.reload_duration {
                        needs_render = true;
                    } else {
                        st.reload_start_time = None;
                        needs_render = true;
                    }
                }

                drop(st);

                if needs_render {
                    do_render(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if state_ptr == 0 { return DefWindowProcW(hwnd, msg, wparam, lparam); }

            match wparam.0 as i32 {
                ID_HOTKEY_TOGGLE_SETTINGS => {
                    if let Ok(h) = FindWindowW(w!("SettingsClass"), None) {
                        if IsWindowVisible(h).as_bool() {
                            let _ = ShowWindow(h, SW_HIDE);
                        } else {
                            let _ = ShowWindow(h, SW_SHOW);
                            let _ = SetForegroundWindow(h);
                        }
                    }
                }
                ID_HOTKEY_CLEAR => {
                    let data = &mut *(state_ptr as *mut WindowData);
                    let mut st = data.state.lock().unwrap();
                    st.kv_pixel_pos = (0, 0);
                    st.c_pixel_pos = (0, 0);
                    st.t_pixel_pos = (0, 0);
                    drop(st);
                    do_render(hwnd);
                }
                ID_HOTKEY_TOGGLE_OVERLAY => {
                    let data = &mut *(state_ptr as *mut WindowData);
                    let mut st = data.state.lock().unwrap();
                    st.visible = !st.visible;
                    drop(st); 
                    do_render(hwnd);
                }
                ID_HOTKEY_RELOAD => {
                    let data = &*(state_ptr as *const WindowData);
                    let mut st = data.state.lock().unwrap();
                    
                    if st.result.valid {
                        st.reload_duration = Duration::from_secs_f32(st.result.time_of_flight as f32);
                        st.reload_start_time = Some(Instant::now());
                        st.timer_label = Some(format!("Time of Flight: {:.2} s", st.result.time_of_flight));
                    }
                    
                    drop(st);
                    do_render(hwnd);
                }
                ID_HOTKEY_SET_KV => {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    let data = &mut *(state_ptr as *mut WindowData);
                    let mut st = data.state.lock().unwrap();
                    unsafe { sync_map_kv_from_settings(&mut st); }
                    st.kv_pixel_pos = (pt.x, pt.y);
                    st.show_coords_helpers = true;
                    if st.c_pixel_pos != (0, 0) && pt.x != st.c_pixel_pos.0 {
                        let dx_px = (st.c_pixel_pos.0 - pt.x).abs() as f64;
                        if dx_px > 1.0 {
                            st.map_scale = 100.0 / dx_px;
                        }
                    }
                    drop(st);
                    do_render(hwnd);

                }
                ID_HOTKEY_SET_CAM => {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    let data = &mut *(state_ptr as *mut WindowData);
                    let mut st = data.state.lock().unwrap();
                    unsafe { sync_map_kv_from_settings(&mut st); }
                    st.c_pixel_pos = (pt.x, pt.y);
                    st.show_coords_helpers = true;
                    if st.kv_pixel_pos != (0, 0) && pt.x != st.kv_pixel_pos.0 {
                        let dx_px = (pt.x - st.kv_pixel_pos.0).abs() as f64;
                        if dx_px > 1.0 {
                            st.map_scale = 100.0 / dx_px;
                        }
                    }
                    drop(st);
                    do_render(hwnd);

                }
                ID_HOTKEY_SET_TARGET => {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    let data = &mut *(state_ptr as *mut WindowData);
                    let mut st = data.state.lock().unwrap();
                    unsafe { sync_map_kv_from_settings(&mut st); }
                    st.t_pixel_pos = (pt.x, pt.y);
                    st.show_coords_helpers = true;
                    
                    let (mx, mz) = parse_map_kv_base(&st.map_kv_str);
                    let dx_px = st.t_pixel_pos.0 - st.kv_pixel_pos.0;
                    let dy_px = st.kv_pixel_pos.1 - st.t_pixel_pos.1;

                    st.target_pos.x = mx + (dx_px as f64 * st.map_scale);
                    st.target_pos.z = mz + (dy_px as f64 * st.map_scale);
                    
                    st.result = run_calculation(&st.self_pos, &st.target_pos, st.gun, st.wind_speed, st.wind_dir, st.obj_height);
                    
                    let settings_hwnd = st.settings_hwnd;
                    drop(st);
                    do_render(hwnd);
                    
                    if settings_hwnd.0 != HWND::default() {
                        let _ = PostMessageW(settings_hwnd.0, WM_UPDATE_SETTINGS_INPUTS, WPARAM(0), LPARAM(0));
                    }
                }
                ID_HOTKEY_SET_TARGET_FROM_COORDS => {
                    let data = &mut *(state_ptr as *mut WindowData);
                    let mut st = data.state.lock().unwrap();

                    if st.kv_pixel_pos == (0, 0) || st.map_scale <= 0.0 {
                        drop(st);
                        return LRESULT(0);
                    }

                    let settings_hwnd = st.settings_hwnd;

                    let get_text = |id: u16| -> String {
                        if settings_hwnd.0 != HWND::default() {
                            if let Ok(h) = GetDlgItem(settings_hwnd.0, id as i32) {
                                let len = GetWindowTextLengthW(h);
                                if len > 0 {
                                    let mut buf = vec![0u16; (len + 1) as usize];
                                    let _ = GetWindowTextW(h, &mut buf);
                                    return String::from_utf16_lossy(&buf[..len as usize]).trim().to_string();
                                }
                            }
                        }
                        String::new()
                    };

                    let x_input = get_text(ID_EDIT_TARG_X).parse::<f64>().unwrap_or(0.0);
                    let z_input = get_text(ID_EDIT_TARG_Z).parse::<f64>().unwrap_or(0.0);

                    let x_offset_m = x_input.rem_euclid(100.0);
                    let z_offset_m = z_input.rem_euclid(100.0);

                    let dx_px = x_offset_m / st.map_scale;
                    let dy_px = z_offset_m / st.map_scale;

                    let kv_x = st.kv_pixel_pos.0 as f64;
                    let kv_y = st.kv_pixel_pos.1 as f64;

                    let t_px_x = kv_x + dx_px;
                    let t_px_y = kv_y - dy_px; 

                    st.t_pixel_pos = (t_px_x.round() as i32, t_px_y.round() as i32);
                    
                    recalc_target_from_pixels(&mut st);

                    drop(st);
                    do_render(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_UPDATE_OVERLAY => {
            do_render(hwnd);
            LRESULT(0)
        }
        WM_PAINT => {
            ValidateRect(hwnd, None);
            LRESULT(0)
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let create_struct = &*(lparam.0 as *const CREATESTRUCTW);
            let state_ptr = create_struct.lpCreateParams as *const Mutex<SharedState>;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

            let h_inst = GetModuleHandleW(None).unwrap();
            let h_instance = HINSTANCE(h_inst.0); 
            
            let mut y = 10;
            
            let create_input = |id: u16, label: &str, default: &str, parent: HWND, hinst: HINSTANCE, cy: &mut i32| {
                let wide_label: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = CreateWindowExW(
                    Default::default(), 
                    w!("STATIC"), 
                    PCWSTR(wide_label.as_ptr()), 
                    WS_CHILD | WS_VISIBLE, 
                    10, *cy, 80, 20, 
                    parent, HMENU::default(), hinst, None
                );

                let wide_default: Vec<u16> = default.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = CreateWindowExW(
                    WS_EX_CLIENTEDGE, 
                    w!("EDIT"), 
                    PCWSTR(wide_default.as_ptr()), 
                    WS_CHILD | WS_VISIBLE | WS_BORDER, 
                    100, *cy, 100, 20, 
                    parent, HMENU(id as *mut c_void), 
                    hinst, None
                );
                *cy += 30;
            };

            let _ = CreateWindowExW(Default::default(), w!("STATIC"), w!("SELF POSITION"), WS_CHILD | WS_VISIBLE, 10, y, 200, 20, hwnd, HMENU::default(), h_instance, None);
            y += 25;
            create_input(ID_EDIT_SELF_X, "X:", "0", hwnd, h_instance, &mut y);
            create_input(ID_EDIT_SELF_Y, "Y:", "0", hwnd, h_instance, &mut y);
            create_input(ID_EDIT_SELF_Z, "Z:", "0", hwnd, h_instance, &mut y);
            
            y += 10;
            let _ = CreateWindowExW(Default::default(), w!("STATIC"), w!("TARGET POSITION"), WS_CHILD | WS_VISIBLE, 10, y, 200, 20, hwnd, HMENU::default(), h_instance, None);
            y += 25;
            create_input(ID_EDIT_TARG_X, "X:", "1000", hwnd, h_instance, &mut y);
            create_input(ID_EDIT_TARG_Y, "Y:", "0", hwnd, h_instance, &mut y);
            create_input(ID_EDIT_TARG_Z, "Z:", "0", hwnd, h_instance, &mut y);

            y += 10;
            create_input(ID_EDIT_OBJ_HEIGHT, "Obj Height:", "0", hwnd, h_instance, &mut y);

            y += 10;
            let _ = CreateWindowExW(Default::default(), w!("STATIC"), w!("MAP SETTINGS"), WS_CHILD | WS_VISIBLE, 10, y, 250, 20, hwnd, HMENU::default(), h_instance, None);
            y += 25;
            create_input(ID_EDIT_MAP_KV, "grid square:", "000000", hwnd, h_instance, &mut y);
            
            let wide_label: Vec<u16> = "Scale (Auto):".encode_utf16().chain(std::iter::once(0)).collect();
            let _ = CreateWindowExW(
                Default::default(), 
                w!("STATIC"), 
                PCWSTR(wide_label.as_ptr()), 
                WS_CHILD | WS_VISIBLE, 
                10, y, 80, 20, 
                hwnd, HMENU::default(), h_instance, None
            );
            let wide_default: Vec<u16> = "0.05".encode_utf16().chain(std::iter::once(0)).collect();
            let style_readonly = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_BORDER.0 | (ES_READONLY as u32));
            let _ = CreateWindowExW(
                WS_EX_CLIENTEDGE, 
                w!("EDIT"), 
                PCWSTR(wide_default.as_ptr()), 
                style_readonly, 
                100, y, 100, 20, 
                hwnd, HMENU(ID_EDIT_MAP_SCALE as *mut c_void), 
                h_instance, None
            );
            y += 30;

            y += 10;
            let _ = CreateWindowExW(Default::default(), w!("STATIC"), w!("WIND SETTINGS"), WS_CHILD | WS_VISIBLE, 10, y, 250, 20, hwnd, HMENU::default(), h_instance, None);
            y += 25;
            create_input(ID_EDIT_WIND_SPEED, "Speed (m/s):", "0", hwnd, h_instance, &mut y);
            create_input(ID_EDIT_WIND_DIR, "Dir (deg):", "0", hwnd, h_instance, &mut y);

            y += 10;
            let _ = CreateWindowExW(Default::default(), w!("STATIC"), w!("GUN TYPE"), WS_CHILD | WS_VISIBLE, 10, y, 80, 20, hwnd, HMENU::default(), h_instance, None);
            
            let combo_style = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | (CBS_DROPDOWNLIST as u32));
            let combo = CreateWindowExW(
                Default::default(), 
                w!("COMBOBOX"), 
                None, 
                combo_style, 
                100, y, 100, 200, 
                hwnd, 
                HMENU(ID_COMBO_GUN as *mut c_void), 
                h_instance, 
                None
            ).unwrap();

            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("122").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("122v").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("125").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("AGS").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("82he1").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("82he2").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("82he3").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("82he4").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("m777he1").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("m777he2").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("m777he3").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("m777he4").0 as isize));
            let _ = SendMessageW(combo, CB_ADDSTRING, WPARAM(0), LPARAM(w!("m777he5").0 as isize));
            let _ = SendMessageW(combo, CB_SETCURSEL, WPARAM(0), LPARAM(0)); 
            y += 30;

            let btn_style = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | (BS_PUSHBUTTON as u32));
            
            let _ = CreateWindowExW(
                Default::default(), 
                w!("BUTTON"), 
                w!("CALCULATE"), 
                btn_style, 
                10, y, 160, 30, 
                hwnd, 
                HMENU(ID_BTN_CALCULATE as *mut c_void), 
                h_instance, 
                None
            );
            y += 40;
            let _ = CreateWindowExW(
                Default::default(), 
                w!("BUTTON"), 
                w!("TOGGLE OVERLAY (=)"), 
                btn_style, 
                10, y, 180, 30, 
                hwnd, 
                HMENU(ID_BTN_TOGGLE_OVERLAY as *mut c_void), 
                h_instance, 
                None
            );
            
            y += 40;

            let _ = CreateWindowExW(
                Default::default(), 
                w!("BUTTON"), 
                w!("EXIT"), 
                btn_style, 
                10, y, 180, 30, 
                hwnd, 
                HMENU(ID_BTN_QUIT as *mut c_void), 
                h_instance, 
                None
            );

            y += 40;
            let _ = CreateWindowExW(Default::default(), w!("STATIC"), w!("Ctrl+1 - Settings\nCtrl+2 - Flight Timer\n= - Toggle Overlay\nCtrl+3 - grid angle(left)\nCtrl+4 - right end of grid (м/px)\nCtrl+5 - place target\nCtrl+6 - Get target pos\nQ+E - Auto Elevate"), WS_CHILD | WS_VISIBLE, 10, y, 300, 150, hwnd, HMENU::default(), h_instance, None);

            LRESULT(0)
        }
        WM_UPDATE_SETTINGS_INPUTS => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Mutex<SharedState>;
            if !state_ptr.is_null() {
                let st = &*state_ptr;
                let guard = st.lock().unwrap();
                
                let set_text = |id: u16, val: f64| {
                    if let Ok(h_edit) = GetDlgItem(hwnd, id as i32) {
                        let txt = format!("{:.5}", val);
                        let wide: Vec<u16> = txt.encode_utf16().chain(std::iter::once(0)).collect();
                        let _ = SetWindowTextW(h_edit, PCWSTR(wide.as_ptr()));
                    }
                };

                set_text(ID_EDIT_TARG_X, guard.target_pos.x);
                set_text(ID_EDIT_TARG_Z, guard.target_pos.z);
                set_text(ID_EDIT_MAP_SCALE, guard.map_scale);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Mutex<SharedState>;
            if state_ptr.is_null() { return LRESULT(0); }
            
            if hiword(wparam.0 as u32) == BN_CLICKED as u16 {
                match loword(wparam.0 as u32) as u16 {
                    ID_BTN_CALCULATE => {
                        let mutex_ref = &*state_ptr;
                        let mut guard = mutex_ref.lock().unwrap();
                        
                        let get_text = |id: u16| -> String {
                            let h_edit = GetDlgItem(hwnd, id as i32).unwrap();
                            let len = GetWindowTextLengthW(h_edit);
                            let mut buf = vec![0u16; (len + 1) as usize];
                            let _ = GetWindowTextW(h_edit, &mut buf);
                            String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string()
                        };

                        guard.self_pos.x = get_text(ID_EDIT_SELF_X).parse().unwrap_or(0.0);
                        guard.self_pos.y = get_text(ID_EDIT_SELF_Y).parse().unwrap_or(0.0);
                        guard.self_pos.z = get_text(ID_EDIT_SELF_Z).parse().unwrap_or(0.0);

                        guard.target_pos.x = get_text(ID_EDIT_TARG_X).parse().unwrap_or(0.0);
                        guard.target_pos.y = get_text(ID_EDIT_TARG_Y).parse().unwrap_or(0.0);
                        guard.target_pos.z = get_text(ID_EDIT_TARG_Z).parse().unwrap_or(0.0);

                        guard.obj_height = get_text(ID_EDIT_OBJ_HEIGHT).parse().unwrap_or(0.0);

                        guard.map_kv_str = get_text(ID_EDIT_MAP_KV);
                        let manual_scale = get_text(ID_EDIT_MAP_SCALE).parse().unwrap_or(guard.map_scale);
                        guard.map_scale = manual_scale; 

                        guard.wind_speed = get_text(ID_EDIT_WIND_SPEED).parse().unwrap_or(0.0);
                        guard.wind_dir = get_text(ID_EDIT_WIND_DIR).parse().unwrap_or(0.0);

                        let h_combo = GetDlgItem(hwnd, ID_COMBO_GUN as i32).unwrap();
                        let sel = SendMessageW(h_combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as i32;
                        
                        guard.gun = match sel {
                            1 => GunType::Gun122v,
                            2 => GunType::Gun125,
                            3 => GunType::GunAGS,
                            4 => GunType::Gun82he1,
                            5 => GunType::Gun82he2,
                            6 => GunType::Gun82he3,
                            7 => GunType::Gun82he4,
                            8 => GunType::Gunm7771,
                            9 => GunType::Gunm7772,
                            10 => GunType::Gunm7773,
                            11 => GunType::Gunm7774,
                            12 => GunType::Gunm7775,
                            _ => GunType::Gun122, // sel = 0
                        };

                        guard.result = run_calculation(&guard.self_pos, &guard.target_pos, guard.gun, guard.wind_speed, guard.wind_dir, guard.obj_height);
                        
                        if guard.overlay_hwnd.0 != HWND::default() {
                            let _ = PostMessageW(guard.overlay_hwnd.0, WM_UPDATE_OVERLAY, WPARAM(0), LPARAM(0));
                        }
                    }
                    ID_BTN_TOGGLE_OVERLAY => {
                        let mutex_ref = &*state_ptr;
                        let mut guard = mutex_ref.lock().unwrap();
                        guard.visible = !guard.visible;
                        if guard.overlay_hwnd.0 != HWND::default() {
                            let _ = PostMessageW(guard.overlay_hwnd.0, WM_UPDATE_OVERLAY, WPARAM(0), LPARAM(0));
                        }
                    }
                    ID_BTN_QUIT => {
                        PostQuitMessage(0);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}