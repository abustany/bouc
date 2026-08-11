import type { AlpineComponent } from "../types/alpinejs";

// Minimum space between the edge of the screen and a popover
const EDGE = 8;

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

interface AppComponentData {
  isAppComponent: typeof isAppComponent;
  currentBooking: PendingBooking | null;
  userId: string | null;

  ensureLoggedIn(): void;
  onUserLoggedIn(userId: string): void;
  onUserLoggedOut(): void;
}

const AppComponent: (
  userId: string | null,
) => AlpineComponent<AppComponentData> = (userId) => ({
  isAppComponent,
  currentBooking: null,
  userId,
  ensureLoggedIn() {
    if (this.userId !== null) return;
    this.$dispatch("show-name-modal");
  },

  onUserLoggedIn(userId: string) {
    this.userId = userId;
  },

  onUserLoggedOut() {
    this.userId = null;
  },
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
  shouldShowPopoverForDay(day: string): boolean;
  placePopover(day: string): void;
  pendingBookingDayClass(
    day: string,
  ): "booking-start" | "booking-middle" | "booking-end" | "";
  startBooking(day: string): void;
  editBooking(b: PendingBooking): void;
  booking: PendingBooking | null;
}

const CalendarComponent: () => AlpineComponent<CalendarComponentData> = () => ({
  state: {
    kind: "init",
    hoveredDay: null,
    popoverDay: null,
    hidePopoverTimeout: null,
  },

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
  },

  onDayMouseEnter(day: string) {
    if (this.state.kind === "init") {
      if (this.state.hidePopoverTimeout) {
        clearTimeout(this.state.hidePopoverTimeout);
      }

      this.state.hoveredDay = day;
      this.state.popoverDay = day;
    } else if (this.state.kind === "pickingDays") {
      this.state.booking.endDay = day;
    }
  },

  onDayMouseLeave() {
    if (this.state.kind === "init") {
      this.state.hidePopoverTimeout = setTimeout(() => {
        if (this.state.kind === "init") this.state.popoverDay = null;
      }, 200);

      this.state.hoveredDay = null;
    }
  },

  onDayClick(day: string) {
    if (this.state.kind === "init") {
      this.onDayMouseEnter(day);
    } else if (this.state.kind === "pickingDays") {
      this.state = {
        kind: "completingBooking",
        booking: this.state.booking,
      };

      asAppChild(this).currentBooking = this.state.booking;
      asAppChild(this).ensureLoggedIn();
    }
  },

  shouldShowPopoverForDay(day: string): boolean {
    return this.state.kind === "init" && this.state.popoverDay === day;
  },

  placePopover(day) {
    place(this.$refs[`cell-${day}`], this.$refs[`day-popover-${day}`]);
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
    asAppChild(this).currentBooking = b;
  },

  get booking() {
    return asAppChild(this).currentBooking;
  },
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

document.addEventListener("alpine:init", () => {
  Alpine.data("app", (userId: string | null) => AppComponent(userId));
  Alpine.data("calendar", () => CalendarComponent());
  Alpine.data("editBooking", () => EditBookingComponent());
  Alpine.data("nameModal", () => NameModalComponent());
});
