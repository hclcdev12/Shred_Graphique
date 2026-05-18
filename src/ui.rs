/// Module pour l'interface graphique GTK
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CheckButton,
    CssProvider, HeaderBar, Label, ProgressBar, Orientation, ScrolledWindow,
    MessageDialog, ButtonsType, DialogFlags, MessageType, ResponseType
};
use std::rc::Rc;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;

use crate::disk::{Disk, detect_disks};
use crate::shred::{start_shred, ShredStatus, ShredMessage};
use crate::system;
use crate::logger;


/// Structure représentant l'état d'un disque dans l'UI
#[derive(Clone)]
struct DiskWidget {
    disk: Disk,
    checkbox: CheckButton,
    progress_bar: ProgressBar,
    progress_text: Label,
    progress_row: GtkBox,
    status_badge: Button,
    verify_label: Label,
    verify_details_button: Button,
    info_button: Button,
    stop_button: Button,
    container: GtkBox,
}

#[derive(Debug, Clone, PartialEq)]
enum JobState {
    Pending,
    Running,
    Success,
    FailedIo,
    FailedOther,
    Cancelled,
}

#[derive(Debug, Clone)]
enum VerifyState {
    Pending,
    Ok,
    Ko,
    Error,
}

#[derive(Debug, Clone)]
struct JobRecord {
    status: JobState,
    attempts: Vec<String>,
    verify_state: Option<VerifyState>,
    verify_output: Option<String>,
}

/// Structure principale de l'application
pub struct ShredApp {
    app: Application,
}

impl ShredApp {
    pub fn new() -> Self {
        let app = Application::builder()
            .application_id("com.shred.graphique")
            .build();

        ShredApp { app }
    }

    pub fn run(&self) {
        let app = self.app.clone();
        
        app.connect_activate(|app| {
            build_ui(app);
        });

        app.run();
    }
}

/// Construit l'interface utilisateur principale
fn build_ui(app: &Application) {
    // Fenêtre principale
    let window = ApplicationWindow::builder()
        .application(app)
        .default_width(860)
        .default_height(620)
        .build();

    setup_css();

    // ── HeaderBar ───────────────────────────────────────────────────────────
    let header = HeaderBar::new();
    header.set_title_widget(Some(&{
        let lbl = Label::new(Some("ShredDisks – Effacement sécurisé"));
        lbl.set_markup("<b>ShredDisks – Effacement sécurisé</b>");
        lbl
    }));

    let refresh_button = Button::with_label("Rafraîchir");
    refresh_button.set_icon_name("view-refresh-symbolic");
    header.pack_end(&refresh_button);

    let cancel_button = Button::with_label("Annuler");
    cancel_button.set_icon_name("window-close-symbolic");
    header.pack_end(&cancel_button);

    window.set_titlebar(Some(&header));

    // Fermer sur cancel
    let window_for_cancel = window.clone();
    cancel_button.connect_clicked(move |_| {
        window_for_cancel.close();
    });

    // Keepalive polkit : démarrage après authentification, arrêté à la fermeture
    let keepalive_handle: Rc<RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>> =
        Rc::new(RefCell::new(None));
    let keepalive_handle_close = keepalive_handle.clone();
    window.connect_close_request(move |_| {
        if let Some(flag) = keepalive_handle_close.borrow().clone() {
            crate::shred::stop_polkit_keepalive(&flag);
        }
        glib::Propagation::Proceed
    });

    // ── Container principal ─────────────────────────────────────────────────
    let main_box = GtkBox::new(Orientation::Vertical, 10);
    main_box.set_margin_top(14);
    main_box.set_margin_bottom(14);
    main_box.set_margin_start(16);
    main_box.set_margin_end(16);

    // ── Bannière d'avertissement ────────────────────────────────────────────
    let warning_banner = GtkBox::new(Orientation::Horizontal, 10);
    warning_banner.add_css_class("warning-banner");
    warning_banner.set_margin_bottom(6);

    let warning_icon = Label::new(Some("⚠"));
    warning_icon.set_markup("<span size='large'>⚠</span>");
    warning_banner.append(&warning_icon);

    let warning_text = GtkBox::new(Orientation::Vertical, 2);
    let wl1 = Label::new(None);
    wl1.set_markup("<b>ATTENTION :</b> Cette opération effacera <b>DÉFINITIVEMENT</b> et <b>IRRÉMÉDIABLEMENT</b> toutes les données des disques sélectionnés.");
    wl1.set_wrap(true);
    wl1.set_xalign(0.0);
    let wl2 = Label::new(Some("Cette action ne peut PAS être annulée (sauf interruption immédiate du processus)."));
    wl2.set_xalign(0.0);
    wl2.set_wrap(true);
    warning_text.append(&wl1);
    warning_text.append(&wl2);
    warning_text.set_hexpand(true);
    warning_banner.append(&warning_text);

    main_box.append(&warning_banner);

    // ── Ligne sélection globale ─────────────────────────────────────────────
    let select_all_button = CheckButton::with_label("Tout sélectionner");

    let selected_total_label = Label::new(Some("Total sélectionné : 0 Go"));
    selected_total_label.add_css_class("total-label");
    selected_total_label.set_halign(gtk4::Align::End);
    selected_total_label.set_hexpand(true);

    let selection_row = GtkBox::new(Orientation::Horizontal, 10);
    selection_row.set_margin_top(4);
    selection_row.set_margin_bottom(4);
    selection_row.append(&select_all_button);
    selection_row.append(&selected_total_label);
    main_box.append(&selection_row);

    // Zone scrollable pour la liste des disques
    let scrolled_window = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .build();
    // Laisser la liste s'agrandir si l'espace est disponible
    scrolled_window.set_vexpand(true);
    scrolled_window.set_propagate_natural_height(true);

    let disk_list_box = GtkBox::new(Orientation::Vertical, 10);
    scrolled_window.set_child(Some(&disk_list_box));
    main_box.append(&scrolled_window);

    // Détecter les disques
    let disks = detect_disks();
    let disk_widgets = Rc::new(RefCell::new(Vec::new()));
    let select_all_updating = Rc::new(Cell::new(false));
    let job_states: Rc<RefCell<HashMap<String, JobRecord>>> = Rc::new(RefCell::new(HashMap::new()));
    let smart_auth_ok = Rc::new(Cell::new(false));
    let keepalive_handle_for_load = keepalive_handle.clone();

    if disks.is_empty() {
        let no_disk_label = Label::new(Some("❌ Aucun disque détecté"));
        no_disk_label.set_markup("<span foreground='red' size='large'>❌ Aucun disque détecté</span>");
        disk_list_box.append(&no_disk_label);
        select_all_button.set_sensitive(false);
    } else {
        if crate::shred::preauthorize_shred().is_ok() {
            smart_auth_ok.set(true);
            start_keepalive_once(&keepalive_handle_for_load);
        }
        for disk in disks {
            let disk_widget = create_disk_widget(&disk);
            attach_disk_info_button(&window, &disk_widget);
            update_info_button_status(&window, &disk_widget, smart_auth_ok.clone());
            attach_verify_details_button(&window, &job_states, &disk_widget);
            disk_list_box.append(&disk_widget.container);

            disk_widgets.borrow_mut().push(disk_widget);
        }
    }

    // ── Bouton de lancement (bas droite) ────────────────────────────────────
    let start_button = Button::builder()
        .label("Lancer la suppression sécurisée")
        .build();
    start_button.add_css_class("destructive-action");

    let retry_failed_button = Button::with_label("Relancer les échoués");
    retry_failed_button.set_sensitive(false);

    let global_status_label = Label::new(Some(""));

    let bottom_row = GtkBox::new(Orientation::Horizontal, 8);
    bottom_row.set_margin_top(10);
    bottom_row.append(&global_status_label);
    let spacer_bottom = GtkBox::new(Orientation::Horizontal, 0);
    spacer_bottom.set_hexpand(true);
    bottom_row.append(&spacer_bottom);
    bottom_row.append(&retry_failed_button);
    bottom_row.append(&start_button);

    main_box.append(&bottom_row);

    window.set_child(Some(&main_box));

    // Gestion du bouton rafraîchissement
    let disk_list_box_clone = disk_list_box.clone();
    let disk_widgets_clone2 = disk_widgets.clone();
    let select_all_clone = select_all_button.clone();
    let select_all_updating_clone = select_all_updating.clone();
    let window_for_refresh = window.clone();
    let job_states_for_refresh = job_states.clone();
    let smart_auth_ok_for_refresh = smart_auth_ok.clone();
    let keepalive_handle_for_refresh = keepalive_handle.clone();
    let retry_failed_button_for_refresh = retry_failed_button.clone();
    let selected_total_label_for_refresh = selected_total_label.clone();
    
    refresh_button.connect_clicked(move |_| {
        // Vider la liste actuelle
        while let Some(child) = disk_list_box_clone.first_child() {
            disk_list_box_clone.remove(&child);
        }
        
        // Réinitialiser les widgets
        disk_widgets_clone2.borrow_mut().clear();
        job_states_for_refresh.borrow_mut().clear();
        retry_failed_button_for_refresh.set_sensitive(false);
        
        // Redétecter les disques
        let disks = detect_disks();
        
        smart_auth_ok_for_refresh.set(false);
        if crate::shred::preauthorize_shred().is_ok() {
            smart_auth_ok_for_refresh.set(true);
            start_keepalive_once(&keepalive_handle_for_refresh);
        }
        if disks.is_empty() {
            let no_disk_label = Label::new(Some("❌ Aucun disque détecté"));
            no_disk_label.set_markup("<span foreground='red' size='large'>❌ Aucun disque détecté</span>");
            disk_list_box_clone.append(&no_disk_label);
            select_all_clone.set_sensitive(false);
            select_all_clone.set_active(false);
            selected_total_label_for_refresh.set_text("Total sélectionné : 0 Go");
        } else {
            for disk in disks {
                let disk_widget = create_disk_widget(&disk);
                attach_disk_info_button(&window_for_refresh, &disk_widget);
                update_info_button_status(&window_for_refresh, &disk_widget, smart_auth_ok_for_refresh.clone());
                attach_verify_details_button(&window_for_refresh, &job_states_for_refresh, &disk_widget);
                disk_list_box_clone.append(&disk_widget.container);

                disk_widgets_clone2.borrow_mut().push(disk_widget);
            }
            select_all_clone.set_sensitive(true);
            attach_select_all_sync(
                disk_widgets_clone2.clone(),
                select_all_clone.clone(),
                select_all_updating_clone.clone(),
                selected_total_label_for_refresh.clone(),
            );
        }
        
        disk_list_box_clone.queue_draw();
    });

    // Gestion du clic sur le bouton
    let disk_widgets_clone = disk_widgets.clone();
    let window_clone = window.clone();
    let global_status_clone = global_status_label.clone();
    let start_button_clone = start_button.clone();
    let retry_failed_button_clone = retry_failed_button.clone();
    let refresh_button_ref = Rc::new(refresh_button.clone());
    let select_all_ref = Rc::new(select_all_button.clone());
    let selected_total_label_for_start = Rc::new(selected_total_label.clone());
    let select_all_updating_for_start = select_all_updating.clone();
    let job_states_for_start = job_states.clone();
    let keepalive_handle_for_start = keepalive_handle.clone();
    
    start_button.connect_clicked(move |_| {
        let selected_disks = get_selected_disks(&disk_widgets_clone.borrow());
        
        if selected_disks.is_empty() {
            show_error_dialog(&window_clone, "Aucun disque sélectionné", "Veuillez sélectionner au least un disque.");
            return;
        }


        // Vérifier la sécurité des disques
        for (disk, _) in &selected_disks {
            let (can_shred, reason) = disk.disk.can_be_shredded();
            if !can_shred {
                show_error_dialog(
                    &window_clone,
                    &format!("Impossible d'effacer {}", disk.disk.name),
                    &reason
                );
                return;
            }
        }

        // Confirmation finale avec callback asynchrone (GTK4 proper way)
        show_confirmation_dialog_async(
            window_clone.clone(),
            &selected_disks,
            disk_widgets_clone.clone(),
            global_status_clone.clone(),
            start_button_clone.clone(),
            retry_failed_button_clone.clone(),
            refresh_button_ref.as_ref().clone(),
            select_all_ref.as_ref().clone(),
            select_all_updating_for_start.clone(),
            selected_total_label_for_start.as_ref().clone(),
            job_states_for_start.clone(),
            keepalive_handle_for_start.clone(),
        );
    });

    // Relancer les échoués
    let window_for_retry = window.clone();
    let disk_widgets_for_retry = disk_widgets.clone();
    let global_status_for_retry = global_status_label.clone();
    let start_button_for_retry = start_button.clone();
    let refresh_button_for_retry = refresh_button.clone();
    let select_all_for_retry = select_all_button.clone();
    let selected_total_label_for_retry = selected_total_label.clone();
    let select_all_updating_for_retry = select_all_updating.clone();
    let job_states_for_retry = job_states.clone();
    let retry_failed_button_for_retry = retry_failed_button.clone();
    let keepalive_handle_for_retry = keepalive_handle.clone();
    retry_failed_button.connect_clicked(move |_| {
        let states = job_states_for_retry.borrow();
        if states.values().any(|r| r.status == JobState::Running) {
            show_error_dialog(&window_for_retry, "Opérations en cours", "Des opérations sont déjà en cours.");
            return;
        }

        let mut failed_disks = Vec::new();
        for widget in disk_widgets_for_retry.borrow().iter() {
            if let Some(record) = states.get(&widget.disk.name) {
                if record.status == JobState::FailedIo || record.status == JobState::FailedOther {
                    failed_disks.push((widget.clone(), widget.disk.clone()));
                }
            }
        }

        if failed_disks.is_empty() {
            show_error_dialog(&window_for_retry, "Aucun échec", "Aucun disque n'est en échec.");
            return;
        }

        for record in job_states_for_retry.borrow_mut().values_mut() {
            record.verify_state = None;
            record.verify_output = None;
        }

        launch_shred_operations(
            window_for_retry.clone(),
            failed_disks,
            disk_widgets_for_retry.clone(),
            global_status_for_retry.clone(),
            start_button_for_retry.clone(),
            retry_failed_button_for_retry.clone(),
            refresh_button_for_retry.clone(),
            select_all_for_retry.clone(),
            select_all_updating_for_retry.clone(),
            selected_total_label_for_retry.clone(),
            job_states_for_retry.clone(),
            keepalive_handle_for_retry.clone(),
        );
    });

    // Gestion du "Tout sélectionner"
    let disk_widgets_clone3 = disk_widgets.clone();
    let _window_for_select_all = window.clone();
    let select_all_for_handler = select_all_button.clone();
    let select_all_updating_for_handler = select_all_updating.clone();
    let selected_total_label_for_handler = selected_total_label.clone();
    select_all_button.connect_toggled(move |btn| {
        if select_all_updating_for_handler.get() {
            return;
        }

        let widgets = disk_widgets_clone3.borrow();
        let selectable: Vec<&DiskWidget> = widgets
            .iter()
            .filter(|w| w.checkbox.is_sensitive())
            .collect();

        if selectable.is_empty() {
            select_all_updating_for_handler.set(true);
            btn.set_active(false);
            select_all_updating_for_handler.set(false);
            btn.set_sensitive(false);
            return;
        }

        if btn.is_active() {
            for widget in selectable {
                widget.checkbox.set_active(true);
            }
        } else {
            for widget in selectable {
                widget.checkbox.set_active(false);
            }
        }

        update_select_all_state(
            &disk_widgets_clone3.borrow(),
            &select_all_for_handler,
            select_all_updating_for_handler.as_ref(),
            &selected_total_label_for_handler,
        );
    });

    attach_select_all_sync(
        disk_widgets.clone(),
        select_all_button.clone(),
        select_all_updating.clone(),
        selected_total_label.clone(),
    );

    window.present();
}

/// Crée le widget pour un disque
fn create_disk_widget(disk: &Disk) -> DiskWidget {
    let (can_shred, reason) = disk.can_be_shredded();
    let media_type = if disk.is_ssd { "SSD" } else { "HDD" };

    // ── Container carte ──────────────────────────────────────────────────────
    let main_container = GtkBox::new(Orientation::Vertical, 6);
    main_container.add_css_class("disk-card");
    main_container.set_margin_bottom(6);

    // ── Ligne 1 : checkbox + nom + badge type + [stop] + badge statut ────────
    let row1 = GtkBox::new(Orientation::Horizontal, 8);
    row1.set_valign(gtk4::Align::Center);

    let checkbox = CheckButton::new();
    checkbox.set_sensitive(can_shred);
    checkbox.set_valign(gtk4::Align::Center);
    row1.append(&checkbox);

    let name_label = Label::new(Some(&disk.name));
    name_label.add_css_class("disk-name");
    name_label.set_xalign(0.0);
    name_label.set_valign(gtk4::Align::Center);
    row1.append(&name_label);

    // Badge type SMART (info_button) — petit carré coloré
    let info_button = Button::with_label("?");
    info_button.add_css_class("info-unknown");
    info_button.set_valign(gtk4::Align::Center);
    row1.append(&info_button);

    // Spacer
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row1.append(&spacer);

    // Bouton Stop (caché au départ, petit)
    let stop_button = Button::with_label("⏹");
    stop_button.add_css_class("destructive-action");
    stop_button.set_sensitive(false);
    stop_button.set_visible(false);
    stop_button.set_valign(gtk4::Align::Center);
    row1.append(&stop_button);

    // Badge statut — toujours visible
    let status_badge = Button::with_label(if can_shred { "INACTIF" } else { "INDISPONIBLE" });
    status_badge.add_css_class("status-badge");
    status_badge.add_css_class(if can_shred { "status-idle" } else { "status-failed" });
    status_badge.set_sensitive(false);
    status_badge.set_can_focus(false);
    status_badge.set_valign(gtk4::Align::Center);
    row1.append(&status_badge);

    main_container.append(&row1);

    // ── Ligne 2 : sous-titre (type • taille modèle) ─────────────────────────
    let subtitle = if can_shred {
        format!("{} • {} ({})", media_type, disk.size, disk.model)
    } else {
        format!("{} • {} ({}) — {}", media_type, disk.size, disk.model, reason)
    };
    let subtitle_label = Label::new(Some(&subtitle));
    subtitle_label.add_css_class("disk-subtitle");
    subtitle_label.set_xalign(0.0);
    subtitle_label.set_margin_start(28); // aligner sous le nom (après checkbox)
    main_container.append(&subtitle_label);

    // ── Ligne 3 : barre de progression + texte droit (cachée init) ───────────
    let progress_row = GtkBox::new(Orientation::Horizontal, 8);
    progress_row.set_margin_start(28);
    progress_row.set_visible(false);

    let progress_bar = ProgressBar::new();
    progress_bar.set_show_text(false);
    progress_bar.set_hexpand(true);
    progress_bar.set_valign(gtk4::Align::Center);
    progress_row.append(&progress_bar);

    let progress_text = Label::new(None);
    progress_text.add_css_class("progress-text");
    progress_text.set_xalign(1.0);
    progress_text.set_valign(gtk4::Align::Center);
    progress_row.append(&progress_text);

    main_container.append(&progress_row);

    // ── Ligne 4 : vérification (cachée init) ─────────────────────────────────
    let verify_label = Label::new(None);
    verify_label.set_margin_start(28);
    verify_label.set_xalign(0.0);
    verify_label.set_visible(false);
    main_container.append(&verify_label);

    let verify_details_button = Button::with_label("Détails vérification");
    verify_details_button.set_margin_start(28);
    verify_details_button.set_halign(gtk4::Align::Start);
    verify_details_button.set_visible(false);
    verify_details_button.set_sensitive(false);
    main_container.append(&verify_details_button);

    // Mise à jour de la bordure de carte selon la sélection
    let container_for_check = main_container.clone();
    checkbox.connect_toggled(move |cb| {
        if cb.is_active() {
            container_for_check.add_css_class("card-selected");
        } else {
            container_for_check.remove_css_class("card-selected");
        }
    });

    // Référence à progress_row dans le DiskWidget via closure — stocké via progress_bar parent
    // (On expose progress_row via la visibilité de progress_bar)
    // (pas nécessaire, supprimé)

    DiskWidget {
        disk: disk.clone(),
        checkbox,
        progress_bar,
        progress_text,
        progress_row,
        status_badge,
        verify_label,
        verify_details_button,
        info_button,
        stop_button,
        container: main_container,
    }
}

/// Récupère les disques sélectionnés
fn get_selected_disks(widgets: &[DiskWidget]) -> Vec<(DiskWidget, Disk)> {
    widgets
        .iter()
        .filter(|w| w.checkbox.is_active())
        .map(|w| (w.clone(), w.disk.clone()))
        .collect()
}

/// Affiche une boîte de dialogue d'erreur
fn show_error_dialog(window: &ApplicationWindow, title: &str, message: &str) {
    let dialog = MessageDialog::new(
        Some(window),
        DialogFlags::MODAL,
        MessageType::Error,
        ButtonsType::Ok,
        message,
    );
    dialog.set_title(Some(title));
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show();
}

/// Affiche une boîte de dialogue de confirmation (async avec callback)
fn show_confirmation_dialog_async(
    window: ApplicationWindow,
    selected_disks: &[(DiskWidget, Disk)],
    all_widgets: Rc<RefCell<Vec<DiskWidget>>>,
    global_status: Label,
    start_button: Button,
    retry_failed_button: Button,
    refresh_button: Button,
    select_all_button: CheckButton,
    select_all_updating: Rc<Cell<bool>>,
    selected_total_label: Label,
    job_states: Rc<RefCell<HashMap<String, JobRecord>>>,
    keepalive_handle: Rc<RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>>,
) {
    let disk_list = selected_disks
        .iter()
        .map(|(_, disk)| format!("  • {} ({})", disk.name, disk.path))
        .collect::<Vec<_>>()
        .join("\n");

    let message = format!(
        "Vous êtes sur le point d'effacer DÉFINITIVEMENT les disques suivants :\n\n{}\n\n\
         Cette action est IRRÉVERSIBLE.\n\
         Toutes les données seront PERDUES.\n\n\
         Êtes-vous ABSOLUMENT SÛR de vouloir continuer ?",
        disk_list
    );

    let dialog = MessageDialog::new(
        Some(&window),
        DialogFlags::MODAL,
        MessageType::Warning,
        ButtonsType::YesNo,
        &message,
    );
    dialog.set_title(Some("⚠️ CONFIRMATION FINALE"));

    // Faire une copie des disques sélectionnés pour le callback
    let selected_disks_copy: Vec<(DiskWidget, Disk)> = selected_disks
        .iter()
        .map(|(w, d)| (w.clone(), d.clone()))
        .collect();

    // Connecter le callback de réponse
    dialog.connect_response(move |dialog, response| {
        if response == ResponseType::Yes {
            // Désactiver le bouton pendant l'opération
            start_button.set_sensitive(false);
            global_status.set_markup("<span size='large' weight='bold'>🔄 Opération en cours...</span>");

            // Lancer les opérations de shred
            launch_shred_operations(
                window.clone(),
                selected_disks_copy.clone(),
                all_widgets.clone(),
                global_status.clone(),
                start_button.clone(),
                retry_failed_button.clone(),
                refresh_button.clone(),
                select_all_button.clone(),
                select_all_updating.clone(),
                selected_total_label.clone(),
                job_states.clone(),
                keepalive_handle.clone(),
            );
        }
        // Fermer le dialogue
        dialog.close();
    });

    // Afficher le dialogue
    dialog.present();
}

/// Lance les opérations de shred pour les disques sélectionnés
fn launch_shred_operations(
    window: ApplicationWindow,
    selected_disks: Vec<(DiskWidget, Disk)>,
    all_widgets: Rc<RefCell<Vec<DiskWidget>>>,
    global_status: Label,
    start_button: Button,
    retry_failed_button: Button,
    refresh_button: Button,
    select_all_button: CheckButton,
    select_all_updating: Rc<Cell<bool>>,
    selected_total_label: Label,
    job_states: Rc<RefCell<HashMap<String, JobRecord>>>,
    keepalive_handle: Rc<RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>>,
) {
    use crate::shred::ShredHandle;

    // Pré-authentifier pour éviter un prompt par disque
    if let Err(e) = crate::shred::preauthorize_shred() {
        show_error_dialog(
            &window,
            "Autorisation requise",
            &format!("Impossible d'obtenir l'autorisation: {}", e),
        );
        global_status.set_markup("");
        start_button.set_sensitive(true);
        refresh_button.set_sensitive(true);
        select_all_button.set_sensitive(true);
        return;
    } else {
        start_keepalive_once(&keepalive_handle);
    }
    
    // Désactiver le bouton refresh pendant l'opération
    refresh_button.set_sensitive(false);
    select_all_button.set_sensitive(false);
    retry_failed_button.set_sensitive(false);
    
    // Récupérer les noms des disques sélectionnés
    let selected_disk_names: Vec<String> = selected_disks.iter().map(|(_, d)| d.name.clone()).collect();
    
    // Désactiver les disques NON sélectionnés
    for widget in all_widgets.borrow().iter() {
        if !selected_disk_names.contains(&widget.disk.name) {
            widget.checkbox.set_sensitive(false);
            widget.stop_button.set_sensitive(false);
        }
    }
    
    let receivers: HashMap<String, Receiver<ShredMessage>> = HashMap::new();
    let handles: HashMap<String, ShredHandle> = HashMap::new();
    
    let receivers = Rc::new(RefCell::new(receivers));
    let handles = Rc::new(RefCell::new(handles));
    
    for (idx, (widget, disk)) in selected_disks.iter().enumerate() {
        // Afficher la barre de progression et le bouton stop
        widget.progress_row.set_visible(true);
        widget.stop_button.set_visible(true);

        widget.progress_bar.set_fraction(0.0);
        widget.progress_text.set_text("Démarrage...");
        set_status_badge(&widget.status_badge, "EN ATTENTE", "status-pending");
        widget.verify_label.set_visible(false);
        widget.verify_details_button.set_visible(false);
        widget.verify_details_button.set_sensitive(false);
        widget.stop_button.set_sensitive(false);

        // Désactiver la checkbox
        widget.checkbox.set_sensitive(false);

        // Configurer le bouton Stop
        let disk_name = disk.name.clone();
        let handles_clone = handles.clone();
        let all_widgets_clone = all_widgets.clone();
        widget.stop_button.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            if let Some(handle) = handles_clone.borrow().get(&disk_name) {
                match crate::shred::stop_shred(handle) {
                    Ok(()) => {
                        if let Some(widget) = all_widgets_clone
                            .borrow()
                            .iter()
                            .find(|w| w.disk.name == disk_name)
                        {
                            set_status_badge(&widget.status_badge, "ARRÊTÉ", "status-stopped");
                        }
                    }
                    Err(e) => {
                        eprintln!("Erreur lors de l'arrêt du processus: {}", e);
                        btn.set_sensitive(true);
                        if let Some(widget) = all_widgets_clone
                            .borrow()
                            .iter()
                            .find(|w| w.disk.name == disk_name)
                        {
                            set_status_badge(&widget.status_badge, "ARRÊT ANNULÉ", "status-stopped");
                        }
                    }
                }
            }
        });

        // Démarrer shred avec un léger décalage pour éviter des prompts concurrents
        let receivers_clone = receivers.clone();
        let handles_clone = handles.clone();
        let disk_path = disk.path.clone();
        let disk_name_for_start = disk.name.clone();
        let stop_button_clone = widget.stop_button.clone();
        let job_states_for_start = job_states.clone();
        let delay_ms = (idx as u64) * 250;
        glib::timeout_add_local(std::time::Duration::from_millis(delay_ms), move || {
            job_states_for_start.borrow_mut().entry(disk_name_for_start.clone()).and_modify(|record| {
                record.status = JobState::Running;
                record.attempts.push("Démarrage".to_string());
            }).or_insert(JobRecord {
                status: JobState::Running,
                attempts: vec!["Démarrage".to_string()],
                verify_state: None,
                verify_output: None,
            });
            let (rx, handle) = start_shred(disk_path.clone(), disk_name_for_start.clone());
            receivers_clone.borrow_mut().insert(disk_name_for_start.clone(), rx);
            handles_clone.borrow_mut().insert(disk_name_for_start.clone(), handle);
            stop_button_clone.set_sensitive(true);
            glib::ControlFlow::Break
        });
    }
    let completed = Rc::new(RefCell::new(0));
    let total = selected_disks.len();
    let io_error_dialog_shown = Rc::new(RefCell::new(HashMap::<String, bool>::new()));

    // Logger le début des opérations
    let disk_names: Vec<String> = selected_disks.iter().map(|(_, d)| d.name.clone()).collect();
    logger::log_operation_start(&disk_names);

    // Vérifier périodiquement les messages
    let all_widgets_clone = all_widgets.clone();
    let receivers_clone = receivers.clone();
    let completed_clone = completed.clone();
    let global_status_clone = global_status.clone();
    let start_button_clone = start_button.clone();
    let retry_failed_button_clone = retry_failed_button.clone();
    let refresh_button_clone3 = refresh_button.clone();
    let select_all_clone3 = select_all_button.clone();
    let selected_total_label_clone3 = selected_total_label.clone();
    let select_all_updating_clone3 = select_all_updating.clone();
    let selected_disk_names_for_end = selected_disk_names.clone();
    let window_clone = window.clone();
    let io_error_dialog_shown_clone = io_error_dialog_shown.clone();
    let job_states_clone = job_states.clone();

    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        let receivers_mut = receivers_clone.borrow_mut();

        for (disk_name, rx) in receivers_mut.iter() {
            while let Ok(msg) = rx.try_recv() {
                // Trouver le widget correspondant
                if let Some(widget) = all_widgets_clone
                    .borrow()
                    .iter()
                    .find(|w| w.disk.name == *disk_name)
                {
                    match msg.status {
                        ShredStatus::Starting => {
                            widget.progress_text.set_text("Démarrage...");
                            set_status_badge(&widget.status_badge, "EN COURS", "status-running");
                        }
                        ShredStatus::InProgress { progress, current_step, total_steps } => {
                            widget.progress_bar.set_fraction(progress);
                            let step_pct = ((progress * total_steps as f64)
                                - (current_step as f64 - 1.0))
                                * 100.0;
                            widget.progress_text.set_text(&format!(
                                "passage {}/{}... {}%",
                                current_step,
                                total_steps,
                                step_pct.clamp(0.0, 100.0) as u32
                            ));
                            set_status_badge(&widget.status_badge, "EN COURS", "status-running");
                        }
                        ShredStatus::Completed => {
                            widget.progress_bar.set_fraction(1.0);
                            widget.progress_text.set_text("100%");
                            set_status_badge(&widget.status_badge, "TERMINÉ", "status-done");
                            widget.stop_button.set_sensitive(false);
                            *completed_clone.borrow_mut() += 1;

                            logger::log_disk_result(disk_name, true, "Succès");
                            update_job_state(&job_states_clone, disk_name, JobState::Success, "Succès");
                            start_verification(
                                window_clone.clone(),
                                widget.disk.path.clone(),
                                disk_name.clone(),
                                widget.verify_label.clone(),
                                widget.verify_details_button.clone(),
                                job_states_clone.clone(),
                            );
                        }
                        ShredStatus::Stopped => {
                            set_status_badge(&widget.status_badge, "ARRÊTÉ", "status-stopped");
                            widget.stop_button.set_sensitive(false);
                            *completed_clone.borrow_mut() += 1;

                            logger::log_disk_result(disk_name, false, "Arrêté par l'utilisateur");
                            update_job_state(&job_states_clone, disk_name, JobState::Cancelled, "Arrêté par l'utilisateur");
                            set_verification_message(&widget.verify_label, "Vérification : non effectuée", "orange");
                        }
                        ShredStatus::FailedIoError { ref message, ref stderr } => {
                            widget.progress_text.set_text("Échec");
                            set_status_badge(&widget.status_badge, "ÉCHEC E/S", "status-failed");
                            widget.stop_button.set_sensitive(false);
                            *completed_clone.borrow_mut() += 1;

                            let log_message = if stderr.trim().is_empty() {
                                message.clone()
                            } else {
                                format!("{}: {}", message, stderr.trim())
                            };
                            logger::log_disk_result(disk_name, false, &log_message);
                            update_job_state(&job_states_clone, disk_name, JobState::FailedIo, &log_message);
                            set_verification_message(&widget.verify_label, "Vérification : non effectuée", "red");

                            let mut shown = io_error_dialog_shown_clone.borrow_mut();
                            if !shown.get(disk_name).copied().unwrap_or(false) {
                                shown.insert(disk_name.clone(), true);
                                let dialog_message = if stderr.trim().is_empty() {
                                    format!("Le disque {} a rencontré une erreur d'entrée/sortie.\nLe processus a été interrompu automatiquement.", disk_name)
                                } else {
                                    format!("Le disque {} a rencontré une erreur d'entrée/sortie.\nLe processus a été interrompu automatiquement.\n\nDétails:\n{}", disk_name, stderr.trim())
                                };
                                show_error_dialog(&window_clone, "Échec shred (erreur E/S)", &dialog_message);
                            }
                        }
                        ShredStatus::FailedOther { ref message, ref stderr } => {
                            widget.progress_text.set_text("Échec");
                            set_status_badge(&widget.status_badge, "ÉCHEC", "status-failed");
                            widget.stop_button.set_sensitive(false);
                            *completed_clone.borrow_mut() += 1;

                            let log_message = if stderr.trim().is_empty() {
                                message.clone()
                            } else {
                                format!("{}: {}", message, stderr.trim())
                            };
                            logger::log_disk_result(disk_name, false, &log_message);
                            update_job_state(&job_states_clone, disk_name, JobState::FailedOther, &log_message);
                            set_verification_message(&widget.verify_label, "Vérification : non effectuée", "red");
                        }
                    }
                }
            }
        }

        // Vérifier si tout est terminé
        let completed_count = *completed_clone.borrow();
        if completed_count == total {
            global_status_clone.set_markup(&format!(
                "<b>✅ Toutes les opérations sont terminées ({}/{})</b>",
                completed_count, total
            ));
            start_button_clone.set_sensitive(true);
            refresh_button_clone3.set_sensitive(true);
            select_all_clone3.set_sensitive(true);
            update_retry_button_state(&job_states_clone, &retry_failed_button_clone);
            
            // Réactiver les disques NON sélectionnés
            for widget in all_widgets_clone.borrow().iter() {
                if !selected_disk_names_for_end.contains(&widget.disk.name) {
                    widget.checkbox.set_sensitive(true);
                    widget.stop_button.set_sensitive(true);
                }
            }
            
            logger::log_operation_end();

            update_select_all_state(
                &all_widgets_clone.borrow(),
                &select_all_clone3,
                select_all_updating_clone3.as_ref(),
                &selected_total_label_clone3,
            );
            
            glib::ControlFlow::Break
        } else {
            global_status_clone.set_markup(&format!(
                "🔄 En cours... ({}/{})",
                completed_count, total
            ));
            update_retry_button_state(&job_states_clone, &retry_failed_button_clone);
            glib::ControlFlow::Continue
        }
    });
}

fn update_job_state(job_states: &Rc<RefCell<HashMap<String, JobRecord>>>, disk_name: &str, status: JobState, message: &str) {
    let mut states = job_states.borrow_mut();
    let entry = states.entry(disk_name.to_string()).or_insert(JobRecord {
        status: JobState::Pending,
        attempts: Vec::new(),
        verify_state: None,
        verify_output: None,
    });
    entry.status = status;
    entry.attempts.push(message.to_string());
}

fn update_retry_button_state(job_states: &Rc<RefCell<HashMap<String, JobRecord>>>, retry_button: &Button) {
    let states = job_states.borrow();
    let has_failed = states.values().any(|record| record.status == JobState::FailedIo || record.status == JobState::FailedOther);
    retry_button.set_sensitive(has_failed);
}

fn set_verification_message(label: &Label, message: &str, color: &str) {
    label.set_visible(true);
    label.set_markup(&format!("<span foreground='{}'>{}</span>", color, message));
}

fn start_verification(
    window: ApplicationWindow,
    disk_path: String,
    disk_name: String,
    verify_label: Label,
    verify_details_button: Button,
    job_states: Rc<RefCell<HashMap<String, JobRecord>>>,
) {
    set_verification_message(&verify_label, "Vérification: en cours...", "blue");
    {
        let mut states = job_states.borrow_mut();
        if let Some(record) = states.get_mut(&disk_name) {
            record.verify_state = Some(VerifyState::Pending);
            record.verify_output = None;
        }
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = system::verify_with_blkid(&disk_path);
        let _ = sender.send(result);
    });

    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(result) => {
                match result {
                    Ok(result) => {
                        if result.ok {
                            set_verification_message(&verify_label, "Vérification: OK", "green");
                            let mut states = job_states.borrow_mut();
                            if let Some(record) = states.get_mut(&disk_name) {
                                record.verify_state = Some(VerifyState::Ok);
                                record.verify_output = None;
                            }
                        } else {
                            set_verification_message(&verify_label, "Vérification: KO", "red");
                            verify_details_button.set_visible(true);
                            verify_details_button.set_sensitive(true);
                            let output = result.output.clone();
                            let mut states = job_states.borrow_mut();
                            if let Some(record) = states.get_mut(&disk_name) {
                                record.verify_state = Some(VerifyState::Ko);
                                record.verify_output = Some(output.clone());
                            }
                        }
                    }
                    Err(err) => {
                        set_verification_message(&verify_label, "Vérification: erreur", "orange");
                        let mut states = job_states.borrow_mut();
                        if let Some(record) = states.get_mut(&disk_name) {
                            record.verify_state = Some(VerifyState::Error);
                            record.verify_output = Some(err.clone());
                        }
                        show_error_dialog(&window, "Erreur vérification", &err);
                    }
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        }
    });
}

fn show_info_dialog(window: &ApplicationWindow, title: &str, message: &str) {
    let dialog = MessageDialog::new(
        Some(window),
        DialogFlags::MODAL,
        MessageType::Info,
        ButtonsType::Ok,
        message,
    );
    dialog.set_title(Some(title));
    dialog.set_use_markup(true);
    dialog.set_markup(message);
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show();
}

fn setup_css() {
    let provider = CssProvider::new();
    provider.load_from_data(
        "/* ── Cartes disques ──────────────────────────────────────────── */
         .disk-card {
             border-radius: 8px;
             border: 2px solid alpha(@theme_fg_color, 0.12);
             padding: 12px 14px;
             margin-bottom: 4px;
         }
         .disk-card.card-selected {
             border-color: @theme_selected_bg_color;
         }
         /* ── Bannière avertissement ──────────────────────────────────── */
         .warning-banner {
             border-radius: 6px;
             padding: 10px 14px;
             background-color: alpha(#e67e22, 0.15);
             border: 1px solid alpha(#e67e22, 0.65);
         }
         /* ── Badge SMART (bouton type) ──────────────────────────────── */
         button.info-good,
         button.info-good:hover,
         button.info-good:active {
             background: #27ae60; color: #ffffff; border: none;
             border-radius: 4px; padding: 1px 7px;
             font-size: 11px; font-weight: bold;
             min-height: 0; min-width: 20px; box-shadow: none;
         }
         button.info-warn,
         button.info-warn:hover,
         button.info-warn:active {
             background: #f39c12; color: #000000; border: none;
             border-radius: 4px; padding: 1px 7px;
             font-size: 11px; font-weight: bold;
             min-height: 0; min-width: 20px; box-shadow: none;
         }
         button.info-bad,
         button.info-bad:hover,
         button.info-bad:active {
             background: #e74c3c; color: #ffffff; border: none;
             border-radius: 4px; padding: 1px 7px;
             font-size: 11px; font-weight: bold;
             min-height: 0; min-width: 20px; box-shadow: none;
         }
         button.info-unknown,
         button.info-unknown:hover,
         button.info-unknown:active {
             background: #7f8c8d; color: #ffffff; border: none;
             border-radius: 4px; padding: 1px 7px;
             font-size: 11px; font-weight: bold;
             min-height: 0; min-width: 20px; box-shadow: none;
         }
         /* ── Badge statut ────────────────────────────────────────────── */
         button.status-badge {
             border-radius: 4px;
             padding: 2px 14px;
             font-size: smaller;
             font-weight: bold;
             min-height: 0;
         }
         button.status-badge.status-idle,
         button.status-badge.status-idle:hover,
         button.status-badge.status-idle:active {
             background: alpha(@theme_fg_color, 0.12);
             color: @theme_fg_color;
             border: none; box-shadow: none;
         }
         button.status-badge.status-pending,
         button.status-badge.status-pending:hover,
         button.status-badge.status-pending:active {
             background: alpha(@theme_selected_bg_color, 0.4);
             color: @theme_fg_color;
             border: none; box-shadow: none;
         }
         button.status-badge.status-running,
         button.status-badge.status-running:hover,
         button.status-badge.status-running:active {
             background: @theme_selected_bg_color;
             color: #ffffff;
             border: none; box-shadow: none;
         }
         button.status-badge.status-done,
         button.status-badge.status-done:hover,
         button.status-badge.status-done:active {
             background: #27ae60; color: #ffffff;
             border: none; box-shadow: none;
         }
         button.status-badge.status-stopped,
         button.status-badge.status-stopped:hover,
         button.status-badge.status-stopped:active {
             background: #e67e22; color: #ffffff;
             border: none; box-shadow: none;
         }
         button.status-badge.status-failed,
         button.status-badge.status-failed:hover,
         button.status-badge.status-failed:active {
             background: #e74c3c; color: #ffffff;
             border: none; box-shadow: none;
         }
         /* ── Barre de progression fine ──────────────────────────────── */
         progressbar > trough { min-height: 5px; }
         progressbar > trough > progress {
             min-height: 5px;
             background-color: @theme_selected_bg_color;
         }
         /* ── Labels typo ────────────────────────────────────────────── */
         .disk-name   { font-weight: bold; font-size: larger; }
         .disk-subtitle { font-size: smaller; opacity: 0.65; }
         .progress-text { font-size: smaller; opacity: 0.75; }
         .total-label   { font-size: smaller; opacity: 0.80; }
        ",
    );

    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn set_info_button_style(button: &Button, style: &str) {
    for class in ["info-good", "info-warn", "info-bad", "info-unknown"] {
        button.remove_css_class(class);
    }
    button.add_css_class(style);
    let label = match style {
        "info-good"    => "i",
        "info-warn"    => "w",
        "info-bad"     => "!",
        _              => "?",
    };
    button.set_label(label);
}

fn set_status_badge(badge: &Button, text: &str, css_class: &str) {
    for class in ["status-idle", "status-pending", "status-running", "status-done", "status-stopped", "status-failed"] {
        badge.remove_css_class(class);
    }
    badge.add_css_class(css_class);
    badge.set_label(text);
}

fn start_keepalive_once(
    handle: &Rc<RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>>,
) {
    if handle.borrow().is_none() {
        let flag = crate::shred::start_polkit_keepalive(10800);
        *handle.borrow_mut() = Some(flag);
    }
}

fn escape_markup(text: &str) -> String {
    glib::markup_escape_text(text).to_string()
}

fn attach_disk_info_button(window: &ApplicationWindow, disk_widget: &DiskWidget) {
    let window = window.clone();
    let disk_path = disk_widget.disk.path.clone();
    let disk_name = disk_widget.disk.name.clone();
    let button = disk_widget.info_button.clone();
    let button_for_click = button.clone();

    button.connect_clicked(move |_| {
        let button_clone = button_for_click.clone();
        button_clone.set_sensitive(false);
        let (sender, receiver) = std::sync::mpsc::channel();
        let disk_path = disk_path.clone();
        std::thread::spawn(move || {
            let result = system::get_smart_info(&disk_path);
            let _ = sender.send(result);
        });

        let window_clone = window.clone();
        let disk_name_clone = disk_name.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            match receiver.try_recv() {
                Ok(result) => {
                    button_clone.set_sensitive(true);
                    match result {
                        Ok(info) => {
                            let (summary_label, summary_color) = match info.summary {
                                system::SmartSummary::Good => ("Bon", "green"),
                                system::SmartSummary::PreFail => ("Pré-fail", "orange"),
                                system::SmartSummary::Bad => ("Mauvais", "red"),
                            };

                            let attrs = if info.attributes.is_empty() {
                                "Aucun attribut clé trouvé".to_string()
                            } else {
                                info.attributes
                                    .iter()
                                    .map(|attr| {
                                        let label = match attr.name.as_str() {
                                            "Reallocated_Sector_Ct" => "Secteurs réalloués",
                                            "Current_Pending_Sector" => "Secteurs en attente",
                                            "Offline_Uncorrectable" => "Erreurs non corrigeables",
                                            "Reported_Uncorrect" => "Erreurs signalées",
                                            "Power_On_Hours" => "Heures de fonctionnement",
                                            "Power_On_Minutes" => "Heures de fonctionnement",
                                            _ => attr.name.as_str(),
                                        };
                                        let label = escape_markup(label);
                                        let raw_value = escape_markup(&attr.raw_value);
                                        let dot_color = match attr.status {
                                            system::SmartAttrStatus::Good => "green",
                                            system::SmartAttrStatus::Warn => "orange",
                                            system::SmartAttrStatus::Bad => "red",
                                        };
                                        if let Some(threshold) = &attr.threshold {
                                            let threshold = escape_markup(threshold);
                                            format!("<span foreground='{}'>●</span> {} = {} (seuil {})", dot_color, label, raw_value, threshold)
                                        } else {
                                            format!("<span foreground='{}'>●</span> {} = {}", dot_color, label, raw_value)
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            };

                            let message = format!(
                                "Disque: {}\nStatut: <span foreground='{}'>●</span> {}\n\nSynthèse:\n{}\n\nAttributs clés:\n{}",
                                escape_markup(&disk_name_clone),
                                summary_color,
                                escape_markup(summary_label),
                                escape_markup(&info.summary_reason),
                                attrs
                            );
                            show_info_dialog(&window_clone, "Infos disque", &message);
                        }
                        Err(err) => {
                            show_error_dialog(&window_clone, "Infos disque", &err);
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => {
                    button_clone.set_sensitive(true);
                    glib::ControlFlow::Break
                }
            }
        });
    });
}

fn update_info_button_status(
    window: &ApplicationWindow,
    disk_widget: &DiskWidget,
    auth_ok: Rc<Cell<bool>>,
) {
    let disk_path = disk_widget.disk.path.clone();
    let button = disk_widget.info_button.clone();
    let window = window.clone();

    set_info_button_style(&button, "info-unknown");

    if !auth_ok.get() {
        return;
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = system::get_smart_info(&disk_path);
        let _ = sender.send(result);
    });

    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(result) => {
                match result {
                    Ok(info) => {
                        let style = match info.summary {
                            system::SmartSummary::Good => "info-good",
                            system::SmartSummary::PreFail => "info-warn",
                            system::SmartSummary::Bad => "info-bad",
                        };
                        set_info_button_style(&button, style);
                    }
                    Err(err) => {
                        set_info_button_style(&button, "info-unknown");
                        show_error_dialog(&window, "Infos disque", &err);
                    }
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        }
    });
}

fn attach_verify_details_button(
    window: &ApplicationWindow,
    job_states: &Rc<RefCell<HashMap<String, JobRecord>>>,
    disk_widget: &DiskWidget,
) {
    let window = window.clone();
    let disk_name = disk_widget.disk.name.clone();
    let job_states = job_states.clone();
    let button = disk_widget.verify_details_button.clone();

    button.connect_clicked(move |_| {
        let output = job_states
            .borrow()
            .get(&disk_name)
            .and_then(|record| record.verify_output.clone())
            .unwrap_or_else(|| "Aucun détail disponible".to_string());

        let message = format!("{}: {}", disk_name, output);
        show_error_dialog(&window, "Vérification KO", &message);
    });
}

fn attach_select_all_sync(
    widgets: Rc<RefCell<Vec<DiskWidget>>>,
    select_all: CheckButton,
    updating: Rc<Cell<bool>>,
    selected_total_label: Label,
) {
    for widget in widgets.borrow().iter() {
        let widgets = widgets.clone();
        let select_all = select_all.clone();
        let updating = updating.clone();
        let selected_total_label = selected_total_label.clone();
        widget.checkbox.connect_toggled(move |_| {
            update_select_all_state(
                &widgets.borrow(),
                &select_all,
                updating.as_ref(),
                &selected_total_label,
            );
        });
    }

    update_select_all_state(
        &widgets.borrow(),
        &select_all,
        updating.as_ref(),
        &selected_total_label,
    );
}

fn update_select_all_state(
    widgets: &[DiskWidget],
    select_all: &CheckButton,
    updating: &Cell<bool>,
    selected_total_label: &Label,
) {
    let selectable: Vec<&DiskWidget> = widgets
        .iter()
        .filter(|w| w.checkbox.is_sensitive())
        .collect();

    if selectable.is_empty() {
        updating.set(true);
        select_all.set_active(false);
        updating.set(false);
        select_all.set_sensitive(false);
        selected_total_label.set_text("Total sélectionné : 0 Go");
        return;
    }

    let all_selected = selectable.iter().all(|w| w.checkbox.is_active());
    updating.set(true);
    select_all.set_active(all_selected);
    updating.set(false);
    select_all.set_sensitive(true);

    let selected_bytes = widgets
        .iter()
        .filter(|w| w.checkbox.is_active())
        .filter_map(|w| parse_size_to_bytes(&w.disk.size))
        .sum::<u64>();

    let text = if selected_bytes >= 1_000_000_000_000 {
        format!("Total sélectionné : {:.2} To", selected_bytes as f64 / 1_000_000_000_000.0)
    } else {
        format!("Total sélectionné : {:.0} Go", selected_bytes as f64 / 1_000_000_000.0)
    };
    selected_total_label.set_text(&text);
}

fn parse_size_to_bytes(size: &str) -> Option<u64> {
    let normalized = size.trim().replace(',', ".");
    if normalized.is_empty() {
        return None;
    }

    let mut number = String::new();
    let mut unit = String::new();

    for ch in normalized.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            number.push(ch);
        } else if !ch.is_whitespace() {
            unit.push(ch);
        }
    }

    let value = number.parse::<f64>().ok()?;
    let unit = unit.to_uppercase();

    let multiplier = match unit.as_str() {
        "" | "B" => 1_f64,
        "K" | "KB" => 1_000_f64,
        "M" | "MB" => 1_000_000_f64,
        "G" | "GB" => 1_000_000_000_f64,
        "T" | "TB" => 1_000_000_000_000_f64,
        "P" | "PB" => 1_000_000_000_000_000_f64,
        "KI" | "KIB" => 1_024_f64,
        "MI" | "MIB" => 1_024_f64.powi(2),
        "GI" | "GIB" => 1_024_f64.powi(3),
        "TI" | "TIB" => 1_024_f64.powi(4),
        "PI" | "PIB" => 1_024_f64.powi(5),
        _ => return None,
    };

    Some((value * multiplier) as u64)
}
