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
  name: string;
  guestCount: number;
}

interface RootData {
  currentBooking: PendingBooking | null;
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

const CalendarComponent: (
  rootData: RootData,
) => AlpineComponent<CalendarComponentData> = (rootData) => ({
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
      rootData.currentBooking = this.state.booking;
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
        name: "",
        guestCount: 1,
      },
    };
  },

  editBooking(b: PendingBooking) {
    rootData.currentBooking = b;
  },

  get booking() {
    return rootData.currentBooking;
  },
});

interface EditBookingComponentData {
  booking: PendingBooking | null;
  modalTitle: string;
  onModalClose(): void;
  onBookingSaved(): void;
}

const EditBookingComponent: (
  rootData: RootData,
) => AlpineComponent<EditBookingComponentData> = (rootData) => ({
  get booking() {
    return rootData.currentBooking;
  },

  get modalTitle() {
    return rootData.currentBooking?.id
      ? window.localizedStrings.booking_modal_title_edit
      : window.localizedStrings.booking_modal_title_new;
  },

  onModalClose() {
    rootData.currentBooking = null;
  },

  onBookingSaved() {
    rootData.currentBooking = null;
  },

  init() {
    const modal = this.$refs.modal as HTMLDialogElement;

    this.$watch("booking", (b) => {
      if (b) {
        modal.showModal();
      } else {
        modal.close();
      }
    });
  },

  close() {
    rootData.currentBooking = null;
  },
});

document.addEventListener("alpine:init", () => {
  const rootData = Alpine.reactive({
    currentBooking: null as PendingBooking | null,
  });
  Alpine.data("calendar", () => CalendarComponent(rootData));
  Alpine.data("editBooking", () => EditBookingComponent(rootData));
});
