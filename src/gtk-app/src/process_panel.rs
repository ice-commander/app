use gtk::glib;
use gtk_process_ui::{ProcessInit, ProcessInput, ProcessModel, ProcessOutput};
use relm4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub struct ProcessPanel {
    pub container: gtk::Box,
    sender: relm4::Sender<ProcessInput>,
    back_cb: Rc<RefCell<Option<Box<dyn Fn()>>>>,
    _controller: Rc<Controller<ProcessModel>>,
}

impl ProcessPanel {
    pub fn new() -> Self {
        let (out_tx, out_rx) = relm4::channel::<ProcessOutput>();
        let controller = ProcessModel::builder()
            .launch(ProcessInit { show_toolbar: true })
            .forward(&out_tx, |o| o);
        let container = controller.widget().clone();
        let sender = controller.sender().clone();
        let back_cb: Rc<RefCell<Option<Box<dyn Fn()>>>> = Rc::new(RefCell::new(None));

        {
            let sender = sender.clone();
            let back_cb = back_cb.clone();
            glib::spawn_future_local(async move {
                while let Some(out) = out_rx.recv().await {
                    match out {
                        ProcessOutput::RequestList => Self::answer_list(&sender),
                        ProcessOutput::Kill(pid) => {
                            if let Err(e) = crate::os_processes::kill_process(pid) {
                                println!("[APP LOG] ❌ Local process kill failed: {}", e);
                            }
                            Self::answer_list(&sender);
                        }
                        ProcessOutput::Back => {
                            if let Some(cb) = back_cb.borrow().as_ref() {
                                cb();
                            }
                        }
                    }
                }
            });
        }

        Self {
            container,
            sender,
            back_cb,
            _controller: Rc::new(controller),
        }
    }

    fn answer_list(sender: &relm4::Sender<ProcessInput>) {
        let (_columns, data) = crate::os_processes::get_process_list();
        let processes: Vec<gtk_process_ui::ProcessInfo> = data
            .into_iter()
            .map(|(pid, name, memory, cpu_usage)| gtk_process_ui::ProcessInfo {
                pid,
                name,
                memory,
                cpu_usage,
            })
            .collect();
        let _ = sender.send(ProcessInput::Update(processes));
    }

    pub fn set_back_callback(&self, cb: impl Fn() + 'static) {
        *self.back_cb.borrow_mut() = Some(Box::new(cb));
    }

    pub fn open_local(&self) {
        Self::answer_list(&self.sender);
    }
}

impl Default for ProcessPanel {
    fn default() -> Self {
        Self::new()
    }
}
