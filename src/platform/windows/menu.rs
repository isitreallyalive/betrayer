use std::any::Any;
use std::mem::zeroed;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow, TrackPopupMenu, 
    SetMenuItemInfoW, GetMenuItemCount, HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, 
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, MENUITEMINFOW, MIIM_BITMAP
};
use windows_sys::Win32::Graphics::Gdi::{DeleteObject, HBITMAP};

use crate::error::{TrayError, TrayResult};
use crate::platform::windows::{encode_wide, error_check, NativeIcon};
use crate::{Menu, MenuItem};

fn set_menu_icon(hmenu: HMENU, item_id: u32, by_position: bool, icon: NativeIcon, bitmaps: &mut Vec<HBITMAP>) -> TrayResult<()> {
    let bitmap = icon.to_bitmap()?;
    
    unsafe {
        let mut menu_info: MENUITEMINFOW = zeroed();
        menu_info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
        menu_info.fMask = MIIM_BITMAP;
        menu_info.hbmpItem = bitmap;

        let by_position_flag = if by_position { 1 } else { 0 };
        error_check(SetMenuItemInfoW(hmenu, item_id, by_position_flag, &menu_info))?;
        
        bitmaps.push(bitmap);
    }
    Ok(())
}

pub struct NativeMenu {
    hmenu: HMENU,
    signals_map: Box<dyn SignalMap>,
    bitmaps: Vec<HBITMAP>,
}

impl NativeMenu {
    pub fn show_on_cursor(&self, hwnd: HWND) -> TrayResult<()> {
        unsafe {
            let mut cursor = zeroed();
            error_check(GetCursorPos(&mut cursor))?;
            error_check(SetForegroundWindow(hwnd))?;
            error_check(TrackPopupMenu(
                self.hmenu,
                TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                cursor.x,
                cursor.y,
                0,
                hwnd,
                null_mut()
            ))?;
        }
        Ok(())
    }

    pub fn map(&self, id: u16) -> Option<&dyn Any> {
        self.signals_map.map(id)
    }
}

impl Drop for NativeMenu {
    fn drop(&mut self) {
        log::trace!("Destroying native menu");
        for bitmap in &self.bitmaps {
            if let Err(err) = error_check(unsafe { DeleteObject(*bitmap as _) }) {
                log::warn!("Failed to destroy menu bitmap: {err}")
            }
        }
        if let Err(err) = error_check(unsafe { DestroyMenu(self.hmenu) }) {
            log::warn!("Failed to destroy native menu: {err}")
        }
    }
}

fn add_all<T>(hmenu: HMENU, signals: &mut Vec<T>, items: Vec<MenuItem<T>>, bitmaps: &mut Vec<HBITMAP>) -> TrayResult<()> {
    for item in items {
        match item {
            MenuItem::Separator => {
                error_check(unsafe { AppendMenuW(hmenu, MF_SEPARATOR, 0, null_mut()) })?;
            }
            MenuItem::Button { name, signal, disabled, checked, icon } => {
                let mut flags = MF_STRING;
                if let Some(true) = checked {
                    flags |= MF_CHECKED;
                }
                if disabled {
                    flags |= MF_GRAYED;
                }
                let wide = encode_wide(&name);
                let menu_id = signals.len();
                error_check(unsafe { AppendMenuW(hmenu, flags, menu_id, wide.as_ptr()) })?;
                
                if let Some(icon) = icon {
                    set_menu_icon(hmenu, menu_id as u32, false, icon.into(), bitmaps)?;
                }
                
                signals.push(signal);
            }
            MenuItem::Menu { name, children, icon } => {
                let submenu = error_check(unsafe { CreatePopupMenu() })?;
                add_all(submenu, signals, children, bitmaps)?;
                let wide = encode_wide(&name);
                let submenu_id = submenu as usize;
                error_check(unsafe { AppendMenuW(hmenu, MF_POPUP, submenu_id, wide.as_ptr()) })?;
                
                if let Some(icon) = icon {
                    let submenu_position = unsafe { 
                        GetMenuItemCount(hmenu) - 1 
                    };
                    
                    if submenu_position >= 0 {
                        if let Err(e) = set_menu_icon(hmenu, submenu_position as u32, true, icon.into(), bitmaps) {
                            log::debug!("Failed to set submenu icon: {}", e);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

impl<T: 'static> TryFrom<Menu<T>> for NativeMenu {
    type Error = TrayError;

    fn try_from(value: Menu<T>) -> Result<Self, Self::Error> {
        log::trace!("Creating new native menu");
        let hmenu = error_check(unsafe { CreatePopupMenu() })?;
        let mut signals = Vec::<T>::new();
        let mut bitmaps = Vec::<HBITMAP>::new();
        add_all(hmenu, &mut signals, value.items, &mut bitmaps)?;
        Ok(Self {
            hmenu,
            signals_map: Box::new(signals),
            bitmaps,
        })
    }
}

trait SignalMap {
    fn map(&self, id: u16) -> Option<&dyn Any>;
}

impl<T: 'static> SignalMap for Vec<T> {
    fn map(&self, id: u16) -> Option<&dyn Any> {
        self.get(id as usize).map(|r| r as _)
    }
}
