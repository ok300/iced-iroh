use std::{sync::Arc, time::Duration};

use iced::{
    Color, Element, Length, Task, Theme, Widget,
    alignment::Horizontal,
    clipboard,
    widget::{button, center_x, center_y, column, container, radio, row, text, text_input},
};
use iced_core::text::Wrapping;
use iroh::{Endpoint, endpoint::presets, protocol::Router};
use iroh_ping::Ping;
use iroh_tickets::{Ticket, endpoint::EndpointTicket};

fn main() -> iced::Result {
    tracing_subscriber::fmt::init();

    iced::application(
        || {
            (
                IcedIrohApp {
                    sidebar_selected: Some(0),
                    ..Default::default()
                },
                Task::perform(run_receiver(), |res| {
                    ReceiverMessage::ReceiverStarted(res.map_err(|e| e.to_string())).into()
                }),
            )
        },
        IcedIrohApp::update,
        IcedIrohApp::view,
    )
    .run()
}

#[derive(Default)]
struct IcedIrohApp {
    receiver_state: ReceiverState,
    ticket_copied: bool,
    sender_input: String,
    pong_output: String,
    sidebar_selected: Option<usize>,
    peers: Vec<EndpointTicket>,
}

#[derive(Default)]
enum ReceiverState {
    #[default]
    Loading,
    // Keep the router alive so the receiver keeps running.
    Ready(Arc<Router>, String),
    Error(String),
}

#[derive(Clone, derive_more::From)]
enum Message {
    Sender(SenderMessage),
    Receiver(ReceiverMessage),
    SidebarRowSelected(usize),
}

#[derive(Clone)]
enum ReceiverMessage {
    ReceiverStarted(Result<Arc<Router>, String>),
    CopyTicket,
    TicketCopied(Result<(), String>),
}

#[derive(Clone)]
enum SenderMessage {
    InputChanged(String),
    SendClicked,
    PingAndSaveClicked,
    PongReceived(Result<Duration, String>),
    PingAndSaved(Result<Duration, String>, EndpointTicket),
}

impl IcedIrohApp {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Receiver(message) => self.update_receiver(message),
            Message::Sender(message) => self.update_sender(message),
            Message::SidebarRowSelected(row) => {
                self.sidebar_selected = Some(row);
                // Clear stale output so results from a previous view don't linger.
                self.pong_output.clear();
                Task::none()
            }
        }
    }

    fn update_receiver(&mut self, message: ReceiverMessage) -> Task<Message> {
        match message {
            ReceiverMessage::ReceiverStarted(res) => self.on_receiver_started(res),
            ReceiverMessage::CopyTicket => self.on_copy_ticket(),
            ReceiverMessage::TicketCopied(res) => self.on_ticket_copied(res),
        }
    }

    fn update_sender(&mut self, message: SenderMessage) -> Task<Message> {
        match message {
            SenderMessage::InputChanged(text) => self.on_input_changed(text),
            SenderMessage::SendClicked => self.on_send_clicked(),
            SenderMessage::PingAndSaveClicked => self.on_ping_and_save_clicked(),
            SenderMessage::PongReceived(res) => self.on_pong_received(res),
            SenderMessage::PingAndSaved(res, ticket) => self.on_ping_and_saved(res, ticket),
        }
    }

    fn on_ping_and_saved(
        &mut self,
        res: Result<Duration, String>,
        ticket: EndpointTicket,
    ) -> Task<Message> {
        if res.is_err() {
            return self.on_pong_received(res);
        }

        let id = ticket.endpoint_addr().id;
        let already_saved = self.peers.iter().any(|p| p.endpoint_addr().id == id);
        if already_saved {
            self.pong_output = format!("err: endpoint {id} is already a peer");
            return Task::none();
        }

        self.peers.push(ticket);
        self.on_pong_received(res)
    }

    fn on_receiver_started(&mut self, res: Result<Arc<Router>, String>) -> Task<Message> {
        self.receiver_state = match res {
            Ok(router) => {
                let ticket = EndpointTicket::new(router.endpoint().addr());
                ReceiverState::Ready(router, ticket.to_string())
            }
            Err(err) => ReceiverState::Error(err),
        };

        Task::none()
    }

    fn on_copy_ticket(&mut self) -> Task<Message> {
        clipboard::write(self.ticket_string()).map(ticket_copied_message)
    }

    fn on_ticket_copied(&mut self, res: Result<(), String>) -> Task<Message> {
        self.ticket_copied = res.is_ok();

        Task::none()
    }

    fn on_input_changed(&mut self, text: String) -> Task<Message> {
        self.sender_input = text;

        Task::none()
    }

    fn on_send_clicked(&mut self) -> Task<Message> {
        let ticket = match self.sidebar_selected {
            Some(i) if i >= 2 => match self.peers.get(i - 2) {
                Some(ticket) => Some(ticket.clone()),
                None => {
                    self.pong_output = format!(
                        "err: selected peer {i} is out of range ({} peer(s) known)",
                        self.peers.len()
                    );
                    None
                }
            },
            _ => self.decode_input_ticket(),
        };

        match ticket {
            Some(ticket) => self.send_ping(ticket, false),
            None => Task::none(),
        }
    }

    fn on_ping_and_save_clicked(&mut self) -> Task<Message> {
        match self.decode_input_ticket() {
            Some(ticket) => self.send_ping(ticket, true),
            None => Task::none(),
        }
    }

    fn decode_input_ticket(&mut self) -> Option<EndpointTicket> {
        match EndpointTicket::decode_string(&self.sender_input) {
            Ok(ticket) => Some(ticket),
            Err(e) => {
                self.pong_output = format!("failed to parse ticket: {e}");
                None
            }
        }
    }

    fn send_ping(&self, ticket: EndpointTicket, save: bool) -> Task<Message> {
        match &self.receiver_state {
            ReceiverState::Ready(router, _) => {
                let send_ep = router.endpoint().clone();

                Task::perform(run_sender(send_ep, ticket.clone()), move |res| {
                    ping_result_message(res.map_err(|e| e.to_string()), ticket, save)
                })
            }
            _ => Task::none(),
        }
    }

    fn on_pong_received(&mut self, res: Result<Duration, String>) -> Task<Message> {
        self.pong_output = match res {
            Ok(rtt) => format!("ping took: {rtt:?} to complete"),
            Err(err) => format!("err: {err}"),
        };

        Task::none()
    }

    fn ticket_string(&self) -> String {
        match &self.receiver_state {
            ReceiverState::Ready(_, ticket) => ticket.clone(),
            _ => String::new(),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let mut sidebar_rows = column![
            radio("This endpoint", 0, self.sidebar_selected, Message::SidebarRowSelected)
                .width(Length::Fill),
            radio("Add peer", 1, self.sidebar_selected, Message::SidebarRowSelected)
                .width(Length::Fill),
        ];
        for (i, peer) in self.peers.iter().enumerate() {
            let id = peer.endpoint_addr().id.to_string();
            let prefix = &id[..id.len().min(8)];
            sidebar_rows = sidebar_rows.push(
                radio(
                    format!("Peer {prefix}..."),
                    i + 2,
                    self.sidebar_selected,
                    Message::SidebarRowSelected,
                )
                .width(Length::Fill)
                .boxed(),
            );
        }

        let sidebar = container(sidebar_rows.spacing(5).width(Length::Fill))
        .width(200)
        .height(Length::Fill)
        .padding(10)
        .style(|theme: &Theme| {
            let tint = theme.palette().primary.base.color;
            container::Style {
                background: Some(Color { a: 0.08, ..tint }.into()),
                ..Default::default()
            }
        });

        let content = container(self.content_view())
            .width(Length::Fill)
            .height(Length::Fill);

        row![sidebar, content]
            .width(Length::Fill)
            .height(Length::Fill)
    }

    fn content_view(&self) -> impl Widget<Message> {
        let body = match self.sidebar_selected {
            Some(0) | None => self.receiver_view(),
            Some(1) => self.sender_view("Ping and Save", SenderMessage::PingAndSaveClicked.into()),
            Some(_) => self.peer_view(),
        };

        center_y(center_x(body))
    }

    fn peer_view(&self) -> Element<'_, Message> {
        column![
            button("Send").on_press(SenderMessage::SendClicked.into()),
            text(&self.pong_output)
        ]
        .spacing(10)
        .align_x(Horizontal::Center)
        .boxed()
    }

    fn sender_view(&self, button_label: &'static str, on_press: Message) -> Element<'_, Message> {
        column![
            text_input("Enter ticket", &self.sender_input)
                .on_input(|text| SenderMessage::InputChanged(text).into())
                .multiline(Some(Wrapping::WordOrGlyph))
                .width(480),
            button(button_label).on_press(on_press),
            text(&self.pong_output)
        ]
        .spacing(10)
        .align_x(Horizontal::Center)
        .boxed()
    }

    fn receiver_view(&self) -> Element<'_, Message> {
        match &self.receiver_state {
            ReceiverState::Ready(_, ticket) => self.ready_receiver_view(ticket),
            ReceiverState::Error(err) => text(format!("error: {err}")).boxed(),
            ReceiverState::Loading => text("Starting receiver...").boxed(),
        }
    }

    fn ready_receiver_view<'a>(&'a self, ticket: &'a str) -> Element<'a, Message> {
        column![
            text("My Ticket"),
            text(ticket).width(480).wrapping(Wrapping::WordOrGlyph),
            button(if self.ticket_copied { "Copied" } else { "Copy" })
                .on_press(ReceiverMessage::CopyTicket.into()),
        ]
        .spacing(10)
        .align_x(Horizontal::Center)
        .boxed()
    }
}

fn ping_result_message(
    res: Result<Duration, String>,
    ticket: EndpointTicket,
    save: bool,
) -> Message {
    if save {
        SenderMessage::PingAndSaved(res, ticket).into()
    } else {
        SenderMessage::PongReceived(res).into()
    }
}

fn ticket_copied_message(result: Result<(), clipboard::Error>) -> Message {
    let res = match result {
        Ok(()) => Ok(()),
        Err(e) => Err(format!("{e:?}")),
    };

    ReceiverMessage::TicketCopied(res).into()
}

async fn run_receiver() -> anyhow::Result<Arc<Router>> {
    let endpoint = Endpoint::bind(presets::N0).await?;
    endpoint.online().await;

    let ping = Ping::new();
    let router = Router::builder(endpoint)
        .accept(iroh_ping::ALPN, ping)
        .spawn();

    Ok(Arc::new(router))
}

async fn run_sender(own_endpoint: Endpoint, ticket: EndpointTicket) -> anyhow::Result<Duration> {
    Ping::new()
        .ping(&own_endpoint, ticket.endpoint_addr().clone())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_changed_updates_sender_input() {
        let mut app = IcedIrohApp::default();

        let _ = app.update(SenderMessage::InputChanged("ticket".into()).into());

        assert_eq!(app.sender_input, "ticket");
    }

    #[test]
    fn pong_received_ok_formats_rtt() {
        let mut app = IcedIrohApp::default();

        let _ = app.update(SenderMessage::PongReceived(Ok(Duration::from_millis(42))).into());

        assert_eq!(app.pong_output, "ping took: 42ms to complete");
    }

    #[test]
    fn pong_received_err_formats_error() {
        let mut app = IcedIrohApp::default();

        let _ = app.update(SenderMessage::PongReceived(Err("boom".into())).into());

        assert_eq!(app.pong_output, "err: boom");
    }

    #[test]
    fn send_clicked_with_invalid_ticket_reports_error() {
        let mut app = IcedIrohApp::default();
        let _ = app.update(SenderMessage::InputChanged("not-a-ticket".into()).into());

        let _ = app.update(SenderMessage::SendClicked.into());

        assert!(app.pong_output.contains("failed to parse ticket"));
    }

    #[test]
    fn send_clicked_with_out_of_range_peer_index_reports_error() {
        let mut app = IcedIrohApp::default();
        let _ = app.update(Message::SidebarRowSelected(5));

        let _ = app.update(SenderMessage::SendClicked.into());

        assert!(app.pong_output.contains("out of range"));
    }

    #[test]
    fn ticket_copied_toggles_flag() {
        let mut app = IcedIrohApp::default();

        let _ = app.update(ReceiverMessage::TicketCopied(Ok(())).into());
        assert!(app.ticket_copied);

        let _ = app.update(ReceiverMessage::TicketCopied(Err("nope".into())).into());
        assert!(!app.ticket_copied);
    }

    #[test]
    fn receiver_started_error_sets_error_state() {
        let mut app = IcedIrohApp::default();

        let _ = app.update(ReceiverMessage::ReceiverStarted(Err("fail".into())).into());

        assert!(
            matches!(&app.receiver_state, ReceiverState::Error(err) if err == "fail")
        );
    }

    #[test]
    fn ticket_string_is_empty_unless_ready() {
        let app = IcedIrohApp::default();

        assert_eq!(app.ticket_string(), "");
    }

    #[test]
    fn copy_ticket_without_ready_state_copies_empty_string() {
        let mut app = IcedIrohApp::default();

        let _ = app.update(ReceiverMessage::CopyTicket.into());

        assert_eq!(app.ticket_string(), "");
    }

    #[test]
    fn ticket_copied_message_maps_result() {
        assert!(matches!(
            ticket_copied_message(Ok(())),
            Message::Receiver(ReceiverMessage::TicketCopied(Ok(())))
        ));
        assert!(matches!(
            ticket_copied_message(Err(clipboard::Error::ClipboardUnavailable)),
            Message::Receiver(ReceiverMessage::TicketCopied(Err(_)))
        ));
    }

    #[test]
    fn sidebar_row_selected_clears_pong_output() {
        let mut app = IcedIrohApp::default();
        let _ = app.update(SenderMessage::PongReceived(Ok(Duration::from_millis(42))).into());
        assert!(!app.pong_output.is_empty());

        let _ = app.update(Message::SidebarRowSelected(1));

        assert_eq!(app.sidebar_selected, Some(1));
        assert_eq!(app.pong_output, "");
    }

    #[test]
    fn view_builds_in_loading_and_error_states() {
        let app = IcedIrohApp::default();
        let _ = app.view();

        let app = IcedIrohApp {
            receiver_state: ReceiverState::Error("x".into()),
            ..Default::default()
        };
        let _ = app.view();
    }
}
