use crate::tools::desktop_uia::{self, RectBounds};
use crate::tools::phantom_session::{self, CachedUiaElement};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiaElementSummary {
    pub id: String,
    pub control_type: String,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub automation_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub value: String,
    pub enabled: bool,
    pub patterns: Vec<String>,
    pub bounds_norm: [i32; 4],
    pub center_norm: [i32; 2],
}

#[derive(Debug, Clone, Default)]
pub struct UiaActionRequest {
    pub op: String,
    pub element_id: Option<String>,
    pub name: Option<String>,
    pub automation_id: Option<String>,
    pub control_type: Option<String>,
    pub value: Option<String>,
    pub scroll_delta: Option<i32>,
}

pub enum UiaActionOutcome {
    Executed(Value),
    FallbackClick {
        cx_norm: i32,
        cy_norm: i32,
        native_hwnd: usize,
        element_name: String,
    },
    FallbackSetText {
        cx_norm: i32,
        cy_norm: i32,
        native_hwnd: usize,
        text: String,
    },
}

#[cfg(windows)]
fn map_control_type_name(id: i32) -> &'static str {
    match id {
        50000 => "Button",
        50001 => "Calendar",
        50002 => "CheckBox",
        50003 => "ComboBox",
        50004 => "Edit",
        50005 => "Hyperlink",
        50006 => "Image",
        50007 => "ListItem",
        50008 => "List",
        50009 => "Menu",
        50010 => "MenuBar",
        50011 => "MenuItem",
        50012 => "ProgressBar",
        50013 => "RadioButton",
        50014 => "ScrollBar",
        50015 => "Slider",
        50016 => "Spinner",
        50017 => "StatusBar",
        50018 => "Tab",
        50019 => "TabItem",
        50020 => "Text",
        50021 => "ToolBar",
        50022 => "ToolTip",
        50023 => "Tree",
        50024 => "TreeItem",
        50025 => "Custom",
        50026 => "Group",
        50028 => "DataGrid",
        50029 => "DataItem",
        50030 => "Document",
        50031 => "SplitButton",
        50032 => "Window",
        50033 => "Pane",
        50034 => "Header",
        50035 => "HeaderItem",
        50036 => "Table",
        50037 => "TitleBar",
        _ => "Control",
    }
}

#[cfg(windows)]
fn is_interactive_control_type(id: i32) -> bool {
    matches!(
        id,
        50000 | 50002 | 50003 | 50004 | 50005 | 50007 | 50011 | 50013 | 50015 | 50019 | 50024
            | 50029 | 50030 | 50031
    )
}

#[cfg(windows)]
unsafe fn create_uia() -> Result<windows::Win32::UI::Accessibility::IUIAutomation, String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::CUIAutomation;

    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        .map_err(|e| format!("Failed to initialize UIAutomation COM: {}", e))
}

#[cfg(windows)]
unsafe fn build_element_cache_request(
    uia: &windows::Win32::UI::Accessibility::IUIAutomation,
) -> Result<windows::Win32::UI::Accessibility::IUIAutomationCacheRequest, String> {
    use windows::Win32::UI::Accessibility::*;

    let req = uia.CreateCacheRequest().map_err(|e| e.to_string())?;
    let props = [
        UIA_ControlTypePropertyId,
        UIA_NamePropertyId,
        UIA_AutomationIdPropertyId,
        UIA_ClassNamePropertyId,
        UIA_IsEnabledPropertyId,
        UIA_BoundingRectanglePropertyId,
        UIA_NativeWindowHandlePropertyId,
        UIA_IsInvokePatternAvailablePropertyId,
        UIA_IsValuePatternAvailablePropertyId,
        UIA_IsTogglePatternAvailablePropertyId,
        UIA_IsSelectionItemPatternAvailablePropertyId,
        UIA_IsExpandCollapsePatternAvailablePropertyId,
        UIA_IsScrollPatternAvailablePropertyId,
        UIA_IsLegacyIAccessiblePatternAvailablePropertyId,
    ];
    for p in props {
        let _ = req.AddProperty(p);
    }
    Ok(req)
}

#[cfg(windows)]
unsafe fn cached_bool_prop(
    el: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    prop: windows::Win32::UI::Accessibility::UIA_PROPERTY_ID,
) -> bool {
    if let Ok(var) = el.GetCachedPropertyValue(prop) {
        let raw_ptr = &var as *const _ as *const u8;
        let vt = u16::from_ne_bytes([*raw_ptr, *raw_ptr.add(1)]);
        if vt == 11 {
            let bval = i16::from_ne_bytes([*raw_ptr.add(8), *raw_ptr.add(9)]);
            return bval != 0;
        }
    }
    false
}

fn norm_coord(val: i32, span: i32) -> i32 {
    if span <= 1 {
        return 0;
    }
    (((val as f64 / span as f64) * 1000.0).round() as i32).clamp(0, 1000)
}

pub fn inspect_window_uia(
    chat_id: Option<&str>,
    hwnd: usize,
    filter: Option<&str>,
    max_elements: usize,
) -> Result<Value, String> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Accessibility::*;

        let root_hwnd = desktop_uia::resolve_root_hwnd(hwnd);
        let win_info = desktop_uia::query_window_info(root_hwnd)
            .ok_or_else(|| "Target window is not valid.".to_string())?;
        let win_x = win_info.bounds.x;
        let win_y = win_info.bounds.y;
        let win_w = win_info.bounds.width.max(1);
        let win_h = win_info.bounds.height.max(1);

        let filter_lower = filter
            .map(|f| f.trim().to_lowercase())
            .filter(|f| !f.is_empty());

        let mut summaries: Vec<UiaElementSummary> = Vec::new();
        let mut cached_list: Vec<CachedUiaElement> = Vec::new();

        if let Ok(uia) = create_uia() {
            if let (Ok(root_el), Ok(true_cond), Ok(cache_req)) = (
                uia.ElementFromHandle(HWND(root_hwnd as *mut core::ffi::c_void)),
                uia.CreateTrueCondition(),
                build_element_cache_request(&uia),
            ) {
                if let Ok(arr) = root_el.FindAllBuildCache(TreeScope_Descendants, &true_cond, &cache_req) {
                    let count = arr.Length().unwrap_or(0).min(1400);
                    let mut seen_sigs = std::collections::HashSet::new();

                    for idx in 0..count {
                        if summaries.len() >= max_elements {
                            break;
                        }
                        let Ok(el) = arr.GetElement(idx) else {
                            continue;
                        };
                        let ctrl_id = el.CachedControlType().map(|c| c.0).unwrap_or(0);
                        let rect = el.CachedBoundingRectangle().unwrap_or_default();
                        let rw = rect.right - rect.left;
                        let rh = rect.bottom - rect.top;
                        if rw < 4 || rh < 4 {
                            continue;
                        }
                        if rect.right <= win_x
                            || rect.bottom <= win_y
                            || rect.left >= win_x + win_w + 40
                            || rect.top >= win_y + win_h + 40
                        {
                            continue;
                        }

                        let name = el.CachedName().map(|b| b.to_string()).unwrap_or_default();
                        let auto_id = el
                            .CachedAutomationId()
                            .map(|b| b.to_string())
                            .unwrap_or_default();
                        let class_name = el
                            .CachedClassName()
                            .map(|b| b.to_string())
                            .unwrap_or_default();
                        let enabled = el.CachedIsEnabled().map(|b| b.as_bool()).unwrap_or(true);
                        let native_hwnd = el
                            .CachedNativeWindowHandle()
                            .map(|h| h.0 as usize)
                            .unwrap_or(0);

                        let mut patterns = Vec::new();
                        if cached_bool_prop(&el, UIA_IsInvokePatternAvailablePropertyId) {
                            patterns.push("invoke".to_string());
                        }
                        let has_val = cached_bool_prop(&el, UIA_IsValuePatternAvailablePropertyId);
                        if has_val {
                            patterns.push("value".to_string());
                        }
                        if cached_bool_prop(&el, UIA_IsTogglePatternAvailablePropertyId) {
                            patterns.push("toggle".to_string());
                        }
                        if cached_bool_prop(&el, UIA_IsSelectionItemPatternAvailablePropertyId) {
                            patterns.push("select".to_string());
                        }
                        if cached_bool_prop(&el, UIA_IsExpandCollapsePatternAvailablePropertyId) {
                            patterns.push("expand".to_string());
                        }
                        if cached_bool_prop(&el, UIA_IsScrollPatternAvailablePropertyId) {
                            patterns.push("scroll".to_string());
                        }
                        if patterns.is_empty()
                            && cached_bool_prop(&el, UIA_IsLegacyIAccessiblePatternAvailablePropertyId)
                        {
                            patterns.push("legacy_action".to_string());
                        }

                        let interactive = is_interactive_control_type(ctrl_id) || !patterns.is_empty();
                        if !interactive && (ctrl_id != 50020 || name.trim().is_empty()) {
                            continue;
                        }
                        if name.trim().is_empty() && auto_id.trim().is_empty() && patterns.is_empty() {
                            continue;
                        }

                        let ctrl_type_str = map_control_type_name(ctrl_id).to_string();
                        if let Some(ref flt) = filter_lower {
                            let hit = name.to_lowercase().contains(flt)
                                || auto_id.to_lowercase().contains(flt)
                                || ctrl_type_str.to_lowercase().contains(flt)
                                || class_name.to_lowercase().contains(flt);
                            if !hit {
                                continue;
                            }
                        }

                        let sig = format!(
                            "{}:{}:{}:{},{},{}x{}",
                            ctrl_id,
                            name.trim(),
                            auto_id.trim(),
                            rect.left,
                            rect.top,
                            rw,
                            rh
                        );
                        if !seen_sigs.insert(sig) {
                            continue;
                        }

                        let mut cur_val = String::new();
                        if has_val && matches!(ctrl_id, 50003 | 50004 | 50015 | 50030) {
                            if let Ok(vp) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                                if let Ok(v) = vp.CurrentValue() {
                                    let s = v.to_string();
                                    cur_val = s.chars().take(140).collect();
                                }
                            }
                        }

                        let nx = norm_coord(rect.left - win_x, win_w);
                        let ny = norm_coord(rect.top - win_y, win_h);
                        let nw = norm_coord(rw, win_w);
                        let nh = norm_coord(rh, win_h);
                        let cx_norm = norm_coord((rect.left + rw / 2) - win_x, win_w);
                        let cy_norm = norm_coord((rect.top + rh / 2) - win_y, win_h);

                        let elem_id = format!("e{}", summaries.len() + 1);
                        cached_list.push(CachedUiaElement {
                            id: elem_id.clone(),
                            control_type: ctrl_type_str.clone(),
                            name: name.trim().chars().take(140).collect(),
                            automation_id: auto_id.trim().to_string(),
                            class_name,
                            native_hwnd,
                            bounds_screen: RectBounds {
                                x: rect.left,
                                y: rect.top,
                                width: rw,
                                height: rh,
                            },
                            cx_norm,
                            cy_norm,
                        });

                        summaries.push(UiaElementSummary {
                            id: elem_id,
                            control_type: ctrl_type_str,
                            name: name.trim().chars().take(140).collect(),
                            automation_id: auto_id.trim().to_string(),
                            value: cur_val,
                            enabled,
                            patterns,
                            bounds_norm: [nx, ny, nw, nh],
                            center_norm: [cx_norm, cy_norm],
                        });
                    }
                }
            }
        }

        if summaries.is_empty() {
            let legacy_ctrls = desktop_uia::find_controls(root_hwnd, filter);
            for ctrl in legacy_ctrls.into_iter().take(max_elements) {
                if ctrl.bounds.width < 4 || ctrl.bounds.height < 4 {
                    continue;
                }
                let nx = norm_coord(ctrl.bounds.x - win_x, win_w);
                let ny = norm_coord(ctrl.bounds.y - win_y, win_h);
                let nw = norm_coord(ctrl.bounds.width, win_w);
                let nh = norm_coord(ctrl.bounds.height, win_h);
                let cx_norm = norm_coord((ctrl.bounds.x + ctrl.bounds.width / 2) - win_x, win_w);
                let cy_norm = norm_coord((ctrl.bounds.y + ctrl.bounds.height / 2) - win_y, win_h);
                let elem_id = format!("e{}", summaries.len() + 1);

                cached_list.push(CachedUiaElement {
                    id: elem_id.clone(),
                    control_type: ctrl.class_name.clone(),
                    name: ctrl.text.clone(),
                    automation_id: String::new(),
                    class_name: ctrl.class_name.clone(),
                    native_hwnd: ctrl.hwnd,
                    bounds_screen: ctrl.bounds,
                    cx_norm,
                    cy_norm,
                });

                summaries.push(UiaElementSummary {
                    id: elem_id,
                    control_type: ctrl.class_name,
                    name: ctrl.text,
                    automation_id: String::new(),
                    value: String::new(),
                    enabled: true,
                    patterns: vec!["hwnd_message".to_string()],
                    bounds_norm: [nx, ny, nw, nh],
                    center_norm: [cx_norm, cy_norm],
                });
            }
        }

        phantom_session::store_uia_cache(chat_id, root_hwnd, cached_list);

        Ok(json!({
            "hwnd": root_hwnd,
            "title": win_info.title,
            "processName": win_info.process_name,
            "windowBounds": win_info.bounds,
            "offscreen": win_info.is_offscreen,
            "elementCount": summaries.len(),
            "elements": summaries,
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = (chat_id, hwnd, filter, max_elements);
        Err("UIAutomation is only supported on Windows.".to_string())
    }
}

pub fn execute_uia_action(
    chat_id: Option<&str>,
    hwnd: usize,
    req: &UiaActionRequest,
) -> Result<UiaActionOutcome, String> {
    #[cfg(windows)]
    unsafe {
        use windows::core::BSTR;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Accessibility::*;

        let root_hwnd = desktop_uia::resolve_root_hwnd(hwnd);
        let win_info = desktop_uia::query_window_info(root_hwnd)
            .ok_or_else(|| "Target window is no longer valid.".to_string())?;
        let win_x = win_info.bounds.x;
        let win_y = win_info.bounds.y;
        let win_w = win_info.bounds.width.max(1);
        let win_h = win_info.bounds.height.max(1);

        let cached_hint = req
            .element_id
            .as_deref()
            .and_then(|eid| phantom_session::lookup_cached_uia_element(chat_id, eid));

        if req.element_id.is_some()
            && cached_hint.is_none()
            && req.name.is_none()
            && req.automation_id.is_none()
        {
            return Err(format!(
                "Element '{}' not found in cached snapshot. Call computer_use(action='snapshot_ui') first or pass name/automation_id.",
                req.element_id.as_deref().unwrap_or("")
            ));
        }

        let want_auto = req
            .automation_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                cached_hint
                    .as_ref()
                    .map(|c| c.automation_id.as_str())
                    .filter(|s| !s.is_empty())
            })
            .map(|s| s.to_lowercase());

        let want_name = req
            .name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                cached_hint
                    .as_ref()
                    .map(|c| c.name.as_str())
                    .filter(|s| !s.is_empty())
            })
            .map(|s| s.to_lowercase());

        let want_type = req
            .control_type
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                cached_hint
                    .as_ref()
                    .map(|c| c.control_type.as_str())
                    .filter(|s| !s.is_empty())
            })
            .map(|s| s.to_lowercase());

        let uia = create_uia()?;
        let root_el = uia
            .ElementFromHandle(HWND(root_hwnd as *mut core::ffi::c_void))
            .map_err(|e| format!("ElementFromHandle failed: {}", e))?;
        let true_cond = uia.CreateTrueCondition().map_err(|e| e.to_string())?;
        let cache_req = build_element_cache_request(&uia)?;
        let arr = root_el
            .FindAllBuildCache(TreeScope_Descendants, &true_cond, &cache_req)
            .map_err(|e| format!("FindAllBuildCache failed: {}", e))?;

        let count = arr.Length().unwrap_or(0).min(1600);
        let mut best_score = -1i32;
        let mut best_el: Option<IUIAutomationElement> = None;
        let mut best_meta: Option<(String, String, usize, i32, i32)> = None;

        for idx in 0..count {
            let Ok(el) = arr.GetElement(idx) else {
                continue;
            };
            let rect = el.CachedBoundingRectangle().unwrap_or_default();
            let rw = rect.right - rect.left;
            let rh = rect.bottom - rect.top;
            if rw < 2 || rh < 2 {
                continue;
            }
            let ctrl_id = el.CachedControlType().map(|c| c.0).unwrap_or(0);
            let ctrl_str = map_control_type_name(ctrl_id).to_lowercase();
            let el_name = el.CachedName().map(|b| b.to_string()).unwrap_or_default();
            let el_name_low = el_name.trim().to_lowercase();
            let el_auto = el
                .CachedAutomationId()
                .map(|b| b.to_string())
                .unwrap_or_default();
            let el_auto_low = el_auto.trim().to_lowercase();

            let mut score = 0i32;
            if let Some(ref wa) = want_auto {
                if !wa.is_empty() {
                    if el_auto_low == *wa {
                        score += 120;
                    } else if el_auto_low.contains(wa) {
                        score += 60;
                    } else if req.automation_id.is_some() {
                        continue;
                    }
                }
            }
            if let Some(ref wn) = want_name {
                if !wn.is_empty() {
                    if el_name_low == *wn {
                        score += 100;
                    } else if el_name_low.contains(wn) {
                        score += 55;
                    } else if req.name.is_some() && want_auto.is_none() {
                        continue;
                    }
                }
            }
            if let Some(ref wt) = want_type {
                if ctrl_str == *wt {
                    score += 35;
                } else if req.control_type.is_some() {
                    continue;
                }
            }
            if let Some(ref hint) = cached_hint {
                let dx = ((rect.left + rw / 2) - (hint.bounds_screen.x + hint.bounds_screen.width / 2)).abs();
                let dy = ((rect.top + rh / 2) - (hint.bounds_screen.y + hint.bounds_screen.height / 2)).abs();
                if dx <= 24 && dy <= 24 {
                    score += 50;
                } else if dx <= 80 && dy <= 80 {
                    score += 20;
                }
            }
            if is_interactive_control_type(ctrl_id) {
                score += 10;
            }

            if score > best_score && score >= 45 {
                let cx_norm = norm_coord((rect.left + rw / 2) - win_x, win_w);
                let cy_norm = norm_coord((rect.top + rh / 2) - win_y, win_h);
                let nhwnd = el
                    .CachedNativeWindowHandle()
                    .map(|h| h.0 as usize)
                    .unwrap_or(0);
                best_score = score;
                best_el = Some(el);
                best_meta = Some((el_name, ctrl_str, nhwnd, cx_norm, cy_norm));
            }
        }

        let Some(target_el) = best_el else {
            if let Some(hint) = cached_hint {
                if matches!(req.op.as_str(), "set_value" | "type") {
                    return Ok(UiaActionOutcome::FallbackSetText {
                        cx_norm: hint.cx_norm,
                        cy_norm: hint.cy_norm,
                        native_hwnd: hint.native_hwnd,
                        text: req.value.clone().unwrap_or_default(),
                    });
                }
                return Ok(UiaActionOutcome::FallbackClick {
                    cx_norm: hint.cx_norm,
                    cy_norm: hint.cy_norm,
                    native_hwnd: hint.native_hwnd,
                    element_name: hint.name,
                });
            }
            return Err("Matching UIA element not found in target window.".to_string());
        };

        let (matched_name, matched_type, native_hwnd, cx_norm, cy_norm) = best_meta.unwrap();
        let op = req.op.trim().to_ascii_lowercase();

        match op.as_str() {
            "invoke" | "click" | "click_element" | "uia_invoke" => {
                if let Ok(inv) = target_el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) {
                    if inv.Invoke().is_ok() {
                        phantom_session::update_virtual_cursor(
                            chat_id,
                            root_hwnd,
                            ((cx_norm as f64 / 1000.0) * win_w as f64) as i32,
                            ((cy_norm as f64 / 1000.0) * win_h as f64) as i32,
                        );
                        return Ok(UiaActionOutcome::Executed(json!({
                            "status": "invoked",
                            "method": "UIA_InvokePattern",
                            "element": matched_name,
                            "controlType": matched_type,
                            "centerNorm": [cx_norm, cy_norm],
                        })));
                    }
                }
                if let Ok(sel) = target_el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) {
                    if sel.Select().is_ok() {
                        return Ok(UiaActionOutcome::Executed(json!({
                            "status": "selected",
                            "method": "UIA_SelectionItemPattern",
                            "element": matched_name,
                            "controlType": matched_type,
                        })));
                    }
                }
                if let Ok(tog) = target_el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
                    if tog.Toggle().is_ok() {
                        return Ok(UiaActionOutcome::Executed(json!({
                            "status": "toggled",
                            "method": "UIA_TogglePattern",
                            "element": matched_name,
                            "controlType": matched_type,
                        })));
                    }
                }
                if let Ok(exp) = target_el.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId) {
                    if exp.Expand().is_ok() {
                        return Ok(UiaActionOutcome::Executed(json!({
                            "status": "expanded",
                            "method": "UIA_ExpandCollapsePattern",
                            "element": matched_name,
                            "controlType": matched_type,
                        })));
                    }
                }
                if let Ok(leg) = target_el.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(UIA_LegacyIAccessiblePatternId) {
                    if leg.DoDefaultAction().is_ok() {
                        return Ok(UiaActionOutcome::Executed(json!({
                            "status": "invoked",
                            "method": "UIA_LegacyIAccessiblePattern",
                            "element": matched_name,
                            "controlType": matched_type,
                        })));
                    }
                }
                Ok(UiaActionOutcome::FallbackClick {
                    cx_norm,
                    cy_norm,
                    native_hwnd,
                    element_name: matched_name,
                })
            }
            "set_value" | "type" => {
                let text = req.value.clone().unwrap_or_default();
                if let Ok(vp) = target_el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                    let bstr = BSTR::from(text.as_str());
                    if vp.SetValue(&bstr).is_ok() {
                        return Ok(UiaActionOutcome::Executed(json!({
                            "status": "value_set",
                            "method": "UIA_ValuePattern",
                            "element": matched_name,
                            "chars": text.chars().count(),
                        })));
                    }
                }
                Ok(UiaActionOutcome::FallbackSetText {
                    cx_norm,
                    cy_norm,
                    native_hwnd,
                    text,
                })
            }
            "toggle" => {
                if let Ok(tog) = target_el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
                    tog.Toggle().map_err(|e| format!("UIA Toggle failed: {}", e))?;
                    return Ok(UiaActionOutcome::Executed(json!({
                        "status": "toggled",
                        "method": "UIA_TogglePattern",
                        "element": matched_name,
                    })));
                }
                Ok(UiaActionOutcome::FallbackClick {
                    cx_norm,
                    cy_norm,
                    native_hwnd,
                    element_name: matched_name,
                })
            }
            "select" => {
                if let Ok(sel) = target_el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) {
                    sel.Select().map_err(|e| format!("UIA Select failed: {}", e))?;
                    return Ok(UiaActionOutcome::Executed(json!({
                        "status": "selected",
                        "method": "UIA_SelectionItemPattern",
                        "element": matched_name,
                    })));
                }
                Ok(UiaActionOutcome::FallbackClick {
                    cx_norm,
                    cy_norm,
                    native_hwnd,
                    element_name: matched_name,
                })
            }
            "expand" => {
                let exp = target_el
                    .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId)
                    .map_err(|e| format!("Element does not support ExpandCollapse: {}", e))?;
                exp.Expand().map_err(|e| format!("Expand failed: {}", e))?;
                Ok(UiaActionOutcome::Executed(json!({
                    "status": "expanded",
                    "element": matched_name,
                })))
            }
            "collapse" => {
                let exp = target_el
                    .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId)
                    .map_err(|e| format!("Element does not support ExpandCollapse: {}", e))?;
                exp.Collapse().map_err(|e| format!("Collapse failed: {}", e))?;
                Ok(UiaActionOutcome::Executed(json!({
                    "status": "collapsed",
                    "element": matched_name,
                })))
            }
            "scroll" => {
                if let Ok(sp) = target_el.GetCurrentPatternAs::<IUIAutomationScrollPattern>(UIA_ScrollPatternId) {
                    let delta = req.scroll_delta.unwrap_or(-240);
                    let vert = if delta < 0 {
                        ScrollAmount_LargeIncrement
                    } else if delta > 0 {
                        ScrollAmount_LargeDecrement
                    } else {
                        ScrollAmount_NoAmount
                    };
                    if sp.Scroll(ScrollAmount_NoAmount, vert).is_ok() {
                        return Ok(UiaActionOutcome::Executed(json!({
                            "status": "scrolled",
                            "method": "UIA_ScrollPattern",
                            "element": matched_name,
                        })));
                    }
                }
                Err("Element does not support UIA ScrollPattern; use action='scroll' with x/y/delta.".to_string())
            }
            other => Err(format!("Unsupported UIA op '{}'.", other)),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (chat_id, hwnd, req);
        Err("UIAutomation is only supported on Windows.".to_string())
    }
}
