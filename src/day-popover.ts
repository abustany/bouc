declare global {
  interface Window {
    localizedStrings: Record<string, string>;
  }
}

let pendingBooking: {
  id?: string;
  startDay: string;
  endDay: string;
  name?: string;
  guestCount?: number;
} | null = null;
let openCell: HTMLElement | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;

function cellOf(node: EventTarget | null): HTMLElement | null {
  return node instanceof Element
    ? node.closest<HTMLElement>("[data-day]")
    : null;
}

function popoverOf(cell: HTMLElement): HTMLElement | null {
  return cell.querySelector<HTMLElement>("[data-day-popover]");
}

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

function hideCellPopover(cell: HTMLElement): void {
  const popover = popoverOf(cell);
  if (popover) popover.hidden = true;
  if (cell === openCell) openCell = null;
}

function showCellPopover(cell: HTMLElement): void {
  if (cell === openCell) return;
  if (openCell) hideCellPopover(openCell);
  const popover = popoverOf(cell);
  if (!popover) return;
  popover.hidden = false;
  place(cell, popover);
  openCell = cell;
}

function schedule(action: () => void, delay: number): void {
  clearTimeout(timer);
  timer = setTimeout(action, delay);
}

function onStartBookingClick(ev: PointerEvent) {
  const cell = cellOf(ev.target);
  if (!cell) return;

  const cellDay = cell.dataset["day"];
  if (!cellDay) return;

  ev.stopPropagation();
  pendingBooking = { startDay: cellDay, endDay: cellDay };
  hideCellPopover(cell);
}

function onEditBookingClick(ev: PointerEvent) {
  if (!ev.target || !(ev.target instanceof HTMLButtonElement)) return;
  ev.stopPropagation();

  const cell = cellOf(ev.target);
  if (!cell) return;

  const bookingId = ev.target.dataset.bookingId;
  const startDay = ev.target.dataset.startDate;
  const endDay = ev.target.dataset.endDate;
  const name = ev.target.dataset.name;
  const guestCountStr = ev.target.dataset.guestCount;
  if (!bookingId || !startDay || !endDay || !name || !guestCountStr) return;

  const guestCount = Number.parseInt(guestCountStr, 10);
  if (Number.isNaN(guestCount)) return;

  pendingBooking = { id: bookingId, startDay, endDay, name, guestCount };
  hideCellPopover(cell);
}

function showCellElementPopover(el: EventTarget | null) {
  if (pendingBooking) return;
  const cell = cellOf(el);
  if (!cell) return;
  // mouseout fired on the way in (cell border, button, popover) has already
  // scheduled a hide for this same day
  clearTimeout(timer);
  showCellPopover(cell);
}

const BookingClasses = ["booking-start", "booking-end", "booking-middle"];

function renderPendingBooking(
  calendars: HTMLElement,
  startDay: string,
  endDay: string,
) {
  for (const n of calendars.querySelectorAll<HTMLElement>(
    "[data-day] > button",
  )) {
    const day = n.parentElement?.dataset.day;
    if (!day || day < startDay) continue;

    for (const c of BookingClasses) n.classList.remove(c);
    if (day === startDay) {
      n.classList.add("booking-start");
    } else if (day > startDay && day < endDay) {
      n.classList.add("booking-middle");
    } else if (day === endDay) {
      n.classList.add("booking-end");
    }
  }
}

function resetPendingBooking(calendars: HTMLElement) {
  if (!pendingBooking) return;
  pendingBooking = null;
  renderPendingBooking(calendars, "0", "0");
}

function setupDayPopover(calendars: HTMLElement) {
  const CLOSE_DELAY = 200;

  // the cell we were tracking belongs to the calendars we are replacing
  openCell = null;

  if (window.matchMedia("(hover: hover)").matches) {
    calendars.addEventListener("mouseover", (event) => {
      showCellElementPopover(event.target);

      if (pendingBooking) {
        const cell = cellOf(event.target);
        if (cell) {
          const day = cell.dataset.day;
          if (day && day >= pendingBooking.startDay)
            pendingBooking.endDay = day;
        }

        renderPendingBooking(
          calendars,
          pendingBooking.startDay,
          pendingBooking.endDay,
        );
      }
    });

    calendars.addEventListener("mouseout", (event) => {
      const cell = cellOf(event.target);
      if (cell) schedule(() => hideCellPopover(cell), CLOSE_DELAY);
    });
  }

  calendars.addEventListener("click", (event) => {
    if (pendingBooking) {
      const cell = cellOf(event.target);
      if (cell) showBookingModal("booking-modal", pendingBooking);
      resetPendingBooking(calendars);
      return;
    }

    showCellElementPopover(event.target);
  });

  calendars.addEventListener("focusin", (event) => {
    showCellElementPopover(event.target);
  });

  for (const node of calendars.querySelectorAll<HTMLElement>(
    "[data-day-popover]",
  )) {
    const startBookingButton = node.querySelector<HTMLButtonElement>(
      "button[data-start-booking]",
    );
    if (!startBookingButton) throw new Error("No book button found");
    startBookingButton.addEventListener("click", onStartBookingClick);

    for (const editBookingButton of node.querySelectorAll<HTMLButtonElement>(
      "button[data-edit-booking]",
    )) {
      editBookingButton.addEventListener("click", onEditBookingClick);
    }
  }
}

// these listen on nodes that outlive the calendars, so they must only ever be
// registered once
function setupPopoverDismiss() {
  document.addEventListener("click", (event) => {
    if (!cellOf(event.target) && openCell) hideCellPopover(openCell);
  });

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && openCell) hideCellPopover(openCell);
    if (event.key === "Escape" && pendingBooking) {
      for (const calendars of document.querySelectorAll<HTMLElement>(
        "[data-calendars]",
      )) {
        resetPendingBooking(calendars);
      }
    }
  });

  window.addEventListener(
    "scroll",
    () => {
      if (openCell) hideCellPopover(openCell);
    },
    { passive: true },
  );

  window.addEventListener("resize", () => {
    if (openCell) hideCellPopover(openCell);
  });
}

// fired by the server via the HX-Trigger header, see src/web.rs
const BOOKING_SAVED_EVENT = "booking-saved";

function setupModals() {
  for (const closeButton of document.querySelectorAll<HTMLButtonElement>(
    "dialog[data-modal] button[data-close-button]",
  )) {
    closeButton.addEventListener("click", (ev) => {
      ev.stopPropagation();
      const node = ev.target;
      if (!(node instanceof Element)) return;
      const modal = node.closest("dialog[data-modal]");
      if (!(modal instanceof HTMLDialogElement)) return;
      modal.close();
    });
  }

  const bookingModal = document.getElementById("booking-modal");
  if (!bookingModal || !(bookingModal instanceof HTMLDialogElement))
    throw new Error("booking-modal not found");

  bookingModal.addEventListener(BOOKING_SAVED_EVENT, () => {
    bookingModal.close();
  });
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

function setFields(
  form: HTMLFormElement,
  values: Record<string, string>,
) {
  for (const [key, value] of Object.entries(values)) {
    const field = form.elements.namedItem(key);
    if (!field) throw new Error(`No form control named "${key}"`);
    if (!("value" in field))
      throw new Error(`Field "${key}" is not a value field`);
    field.value = value;
  }
}

function showBookingModal(
  id: string,
  data: {
    id?: string;
    startDay: string;
    endDay: string;
    name?: string;
    guestCount?: number;
  },
) {
  const modal = document.querySelector<HTMLDialogElement>(
    `dialog#${id}[data-modal]`,
  );
  if (!modal) return;

  const modalTitle = modal.querySelector<HTMLElement>("[data-title]");
  if (modalTitle)
    modalTitle.textContent =
      window.localizedStrings[
        data.id ? "booking_modal_title_edit" : "booking_modal_title_new"
      ];

  const startDateSpan = modal.querySelector<HTMLSpanElement>(
    "span[data-start-date]",
  );
  if (!startDateSpan) return;
  startDateSpan.textContent = formatDate(data.startDay);

  const endDateSpan = modal.querySelector<HTMLSpanElement>(
    "span[data-end-date]",
  );
  if (!endDateSpan) return;
  endDateSpan.textContent = formatDate(data.endDay);

  const form = modal.querySelector<HTMLFormElement>("form");
  if (!form) return;

  form.reset();

  setFields(form, {
    id: data.id ?? "",
    name: data.name ?? "",
    guest_count: data.guestCount?.toString() ?? "1",
    start_date: data.startDay,
    end_date: data.endDay,
  });

  modal.showModal();
}

// htmx fires this on the body once the page is ready, and on every element it
// swaps in afterwards
document.body.addEventListener("htmx:load", (ev) => {
  const node = ev.target;
  if (!(node instanceof Element)) return;

  const calendars = node.matches("[data-calendars]")
    ? node
    : node.querySelector("[data-calendars]");
  if (!(calendars instanceof HTMLElement)) return;

  setupDayPopover(calendars);
});

setupPopoverDismiss();
setupModals();
