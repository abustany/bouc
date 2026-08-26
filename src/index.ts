import type { AlpineComponent } from "../types/alpinejs";

// Minimum space between the edge of the screen and a popover
const EDGE = 8;

// Has to be kept in sync with the `sheet` variant in src/styles.css, which
// turns the popover into a sheet coming from the bottom of the screen
const SheetMedia = window.matchMedia("(width < 400px) and (hover: none)");

function isSheetMode(): boolean {
  return SheetMedia.matches;
}

function place(cell: HTMLElement, popover: HTMLElement): void {
  const anchor = cell.getBoundingClientRect();
  popover.style.top = "0px";
  popover.style.left = "0px";
  const box = popover.getBoundingClientRect();

  // flush against the cell: the popover's own padding provides the visual gap
  let top = anchor.bottom;
  if (top + box.height > window.innerHeight - EDGE) {
    const above = anchor.top - box.height;
    top =
      above >= EDGE
        ? above
        : Math.max(EDGE, window.innerHeight - box.height - EDGE);
  }

  let left = anchor.left + (anchor.width - box.width) / 2;
  left = Math.min(Math.max(EDGE, left), window.innerWidth - box.width - EDGE);

  popover.style.top = `${top}px`;
  popover.style.left = `${left}px`;
}

const DateFormat = new Intl.DateTimeFormat(navigator.language, {
  month: "long",
  day: "numeric",
  year: "numeric",
});

function formatDate(yyyymmdd: string): string {
  if (!yyyymmdd.match(/^\d{8}$/)) throw new Error(`Invalid date: ${yyyymmdd}`);
  return DateFormat.format(
    new Date(
      Number.parseInt(yyyymmdd.slice(0, 4), 10), // year
      Number.parseInt(yyyymmdd.slice(4, 6), 10) - 1, // month (0-indexed)
      Number.parseInt(yyyymmdd.slice(6, 8), 10), // day
    ),
  );
}

declare global {
  interface Window {
    formatDate: typeof formatDate;
  }
}

window.formatDate = formatDate;

interface PendingBooking {
  id?: string;
  startDay: string;
  endDay: string;
  guestCount: number;
}

const isAppComponent = Symbol("app");

type NotificationSubscriptionState = "none" | "pending" | "active" | "disabled";

interface AppComponentData {
  isAppComponent: typeof isAppComponent;
  currentBooking: PendingBooking | null;
  userId: string | null;
  notificationSubscriptionState: NotificationSubscriptionState;

  ensureLoggedIn(): void;
  onUserLoggedIn(userId: string, notificationSubscriptionState: NotificationSubscriptionState): void;
  onUserLoggedOut(): void;
  onNotificationSubscriptionStateChanged(state: NotificationSubscriptionState): void;
}

const AppComponent: (
  userId: string | null,
  notificationSubscriptionState: NotificationSubscriptionState,
) => AlpineComponent<AppComponentData> = (userId, notificationSubscriptionState) => ({
  isAppComponent,
  currentBooking: null,
  userId,
  notificationSubscriptionState,

  ensureLoggedIn() {
    if (this.userId !== null) return;
    this.$dispatch("show-name-modal");
  },

  onUserLoggedIn(userId: string, notificationSubscriptionState: NotificationSubscriptionState) {
    this.userId = userId;
    this.notificationSubscriptionState = notificationSubscriptionState;
  },

  onUserLoggedOut() {
    this.userId = null;
    this.notificationSubscriptionState = "none";
  },

  onNotificationSubscriptionStateChanged(state) {
    this.notificationSubscriptionState = state
  }
});

function asAppChild(obj: unknown): AppComponentData {
  if (
    typeof obj !== "object" ||
    obj === null ||
    !("isAppComponent" in obj) ||
    obj.isAppComponent !== isAppComponent
  ) {
    throw new Error("Not an app child");
  }

  return obj as AppComponentData;
}

interface CalendarComponentData {
  state:
    | {
        kind: "init";
        hoveredDay: string | null;
        popoverDay: string | null;
        hidePopoverTimeout: ReturnType<typeof setTimeout> | null;
      }
    | {
        kind: "pickingDays"; // user clicked "Book", picks days
        booking: PendingBooking;
      }
    | {
        kind: "completingBooking"; // user picked days, completes remaining form fields
        booking: PendingBooking;
      };
  onDayMouseEnter(day: string): void;
  onDayMouseLeave(): void;
  onDayClick(day: string): void;
  onBookingSaved(): void;
  onSubscribeToNotifications(): void;
  instantPopover: boolean;
  justBooked: boolean;
  hintToShow: "pick-day"|"pick-end-day"|"booking-complete" | null;
  shouldShowPopoverForDay(day: string): boolean;
  popoverClasses(day: string): string;
  placePopover(day: string): void;
  openPopover(day: string): void;
  closePopover(): void;
  pendingBookingDayClass(
    day: string,
  ): "booking-start" | "booking-middle" | "booking-end" | "";
  startBooking(day: string): void;
  editBooking(b: PendingBooking): void;
  booking: PendingBooking | null;
  currentUserId: string | null;
}

const CalendarComponent: () => AlpineComponent<CalendarComponentData> = () => ({
  state: {
    kind: "init",
    hoveredDay: null,
    popoverDay: null,
    hidePopoverTimeout: null,
  },

  instantPopover: false,

  init() {
    this.$watch("booking", (b) => {
      if (!b) {
        this.state = {
          kind: "init",
          hoveredDay: null,
          popoverDay: null,
          hidePopoverTimeout: null,
        };
      }
    });

    this.$watch("currentUserId", () => {
      this.justBooked = false;
    });
  },

  onDayMouseEnter(day: string) {
    if (this.state.kind === "init") {
      // sheet opens on click
      if (!isSheetMode()) this.openPopover(day);

      this.state.hoveredDay = day;
    } else if (this.state.kind === "pickingDays") {
      if (day >= this.state.booking.startDay) {
        this.state.booking.endDay = day;
      }
    }
  },

  onDayMouseLeave() {
    if (this.state.kind === "init") {
      // sheet is closed with the close button
      if (!isSheetMode()) {
        this.state.hidePopoverTimeout = setTimeout(() => this.closePopover(), 200);
      }

      this.state.hoveredDay = null;
    }
  },

  onDayClick(day: string) {
    if (this.state.kind === "init") {
      this.openPopover(day);
    } else if (this.state.kind === "pickingDays") {
      this.state = {
        kind: "completingBooking",
        booking: this.state.booking,
      };

      asAppChild(this).currentBooking = this.state.booking;
      asAppChild(this).ensureLoggedIn();
    }
  },

  onBookingSaved() {
    this.justBooked = true;
  },

  onSubscribeToNotifications() {
    this.$dispatch("show-email-modal");
  },

  justBooked: false,

  get hintToShow() {
    if (this.justBooked) return "booking-complete"
    if (this.state.kind === "init") return "pick-day"
    if (this.state.kind === "pickingDays") return "pick-end-day"
    return null
  },

  shouldShowPopoverForDay(day: string): boolean {
    return this.state.kind === "init" && this.state.popoverDay === day;
  },

  popoverClasses(day: string): string {
    // visibility is part of the transition so the sheet stays on screen while
    // it slides back out
    const slide = this.instantPopover
      ? ""
      : "sheet:transition-[translate,visibility] sheet:duration-200 ";

    return this.shouldShowPopoverForDay(day)
      ? `${slide}visible sheet:translate-y-0`
      : `${slide}invisible sheet:translate-y-full`;
  },

  placePopover(day) {
    const popover = this.$refs[`day-popover-${day}`];

    if (isSheetMode()) {
      // coordinates left over from a previous placement would win over the
      // classes pinning the sheet to the bottom of the screen
      popover.style.top = "";
      popover.style.left = "";
      return;
    }

    place(this.$refs[`cell-${day}`], popover);
  },

  openPopover(day: string) {
    if (this.state.kind !== "init") return;

    if (this.state.hidePopoverTimeout) {
      clearTimeout(this.state.hidePopoverTimeout);
      this.state.hidePopoverTimeout = null;
    }

    // one sheet sliding out while another slides in is noise: going straight
    // from a day to the next swaps them without an animation
    this.instantPopover = this.state.popoverDay !== null;
    this.state.popoverDay = day;
  },

  closePopover() {
    if (this.state.kind !== "init") return;

    if (this.state.hidePopoverTimeout) {
      clearTimeout(this.state.hidePopoverTimeout);
      this.state.hidePopoverTimeout = null;
    }

    this.instantPopover = false;
    this.state.popoverDay = null;
  },

  pendingBookingDayClass(day) {
    if (this.state.kind !== "pickingDays") return "";
    if (day === this.state.booking.startDay) return "booking-start";
    if (day > this.state.booking.startDay && day < this.state.booking.endDay)
      return "booking-middle";
    if (day === this.state.booking.endDay) return "booking-end";
    return "";
  },

  startBooking(day: string) {
    if (this.state.kind !== "init") return;
    this.justBooked = false;
    this.state = {
      kind: "pickingDays",
      booking: {
        startDay: day,
        endDay: day,
        guestCount: 1,
      },
    };
  },

  editBooking(b: PendingBooking) {
    this.justBooked = false;
    asAppChild(this).currentBooking = b;
  },

  get booking() {
    return asAppChild(this).currentBooking;
  },

  get currentUserId() {
    return asAppChild(this).userId
  }
});

interface EditBookingComponentData {
  booking: PendingBooking | null;
  shouldShowModal: boolean;
  modalTitle: string;
  onModalClose(): void;
  onBookingSaved(): void;
}

const EditBookingComponent: () => AlpineComponent<EditBookingComponentData> =
  () => ({
    get booking() {
      return asAppChild(this).currentBooking;
    },

    get shouldShowModal() {
      return (
        asAppChild(this).userId !== null &&
        asAppChild(this).currentBooking !== null
      );
    },

    get modalTitle() {
      return asAppChild(this).currentBooking?.id
        ? window.localizedStrings.booking_modal_title_edit
        : window.localizedStrings.booking_modal_title_new;
    },

    onModalClose() {
      asAppChild(this).currentBooking = null;
    },

    onBookingSaved() {
      asAppChild(this).currentBooking = null;
    },

    init() {
      const modal = this.$refs.modal as HTMLDialogElement;

      this.$watch("shouldShowModal", (b) => {
        if (b) {
          modal.showModal();
        } else {
          modal.close();
        }
      });
    },

    close() {
      asAppChild(this).currentBooking = null;
    },
  });

interface NameModalComponentData {
  showModal(): void;
  appUserId: string | null;
}

const NameModalComponent: () => AlpineComponent<NameModalComponentData> =
  () => ({
    showModal() {
      (this.$refs.modal as HTMLDialogElement).showModal();
    },

    get appUserId() {
      return asAppChild(this).userId;
    },

    init() {
      this.$watch("appUserId", (userId) => {
        if (userId !== null) {
          (this.$refs.modal as HTMLDialogElement).close();
        }
      });
    },
  });

interface EmailModalComponentData {
  showModal(): void;
  appNotificationSubscriptionState: NotificationSubscriptionState;
}

const EmailModalComponent: () => AlpineComponent<EmailModalComponentData> =
  () => ({
    showModal() {
      (this.$refs.modal as HTMLDialogElement).showModal();
    },

    get appNotificationSubscriptionState() {
      return asAppChild(this).notificationSubscriptionState;
    },

    init() {
      this.$watch("appNotificationSubscriptionState", (notificationSubscriptionState) => {
        if (notificationSubscriptionState !== "none") {
          (this.$refs.modal as HTMLDialogElement).close();
        }
      });
    },
  });

document.addEventListener("alpine:init", () => {
  Alpine.data("app", (userId: string | null, notificationSubscriptionState: NotificationSubscriptionState) => AppComponent(userId, notificationSubscriptionState));
  Alpine.data("calendar", () => CalendarComponent());
  Alpine.data("editBooking", () => EditBookingComponent());
  Alpine.data("nameModal", () => NameModalComponent());
  Alpine.data("emailModal", () => EmailModalComponent());
});
