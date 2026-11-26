// The viewer's page: it shows one node of one solution file, as /api/node answers it, and moves
// to another by changing the address after the `#`, so that back and forward work. `#new` is
// the form that starts a solve, estimated by /api/estimate as it is filled in, and `#solve` the
// solve running, which /api/solve reports about once a second.
// Amounts arrive in chips, 100 to the big blind, and frequencies as fractions.
"use strict";

const $ = (id) => document.getElementById(id);
const RANKS = "AKQJT98765432";
const SUITS = "shdc";

const state = {
  files: [], folder: "", file: "", path: "", hand: "", show: "strategy", answer: null,
  inputs: null, inputsError: "", poll: 0,
};

// ---- Small helpers -----------------------------------------------------------------------

function make(tag, className, text) {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
}

function bb(chips, digits = 2) {
  if (chips === null || chips === undefined) return "–";
  const value = chips / 100;
  return (Number.isInteger(value) ? String(value) : value.toFixed(digits)) + "bb";
}

function percent(fraction, digits = 1) {
  return fraction === null || fraction === undefined ? "–" : (100 * fraction).toFixed(digits) + "%";
}

function seat(player) {
  return player === "oop" ? "OOP" : "IP";
}

function cardChip(card, tag = "span") {
  const chip = make(tag, "card " + card[1], card[0]);
  chip.setAttribute("aria-label", card);
  chip.append(make("small", "", { s: "♠", h: "♥", d: "♦", c: "♣" }[card[1]]));
  return chip;
}

function token(name) {
  const rest = name.slice(1);
  switch (name[0].toLowerCase()) {
    case "x": return "Check";
    case "c": return "Call";
    case "f": return "Fold";
    case "a": return "All-in";
    case "b": return "Bet " + rest + "%";
    case "r": return "Raise " + rest;
    default: return name;
  }
}

function showError(message) {
  $("banner").textContent = message;
  $("banner").hidden = !message;
}

async function api(url) {
  const response = await fetch(url);
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body && body.error ? body.error : "The server answered " + response.status);
  return body;
}

// ---- Actions and their colours -----------------------------------------------------------

// Each action's colour, and the order the mixes are drawn in: the largest bet first, then the
// check or call, then the fold.
function palette(actions) {
  const sized = actions
    .map((action, index) => ({ action, index }))
    .filter(({ action }) => action.action === "bet" || action.action === "raise")
    .sort((a, b) => a.action.adds - b.action.adds);
  const shades = sized.length === 1 ? ["--act-bet-2"] : ["--act-bet-1", "--act-bet-2", "--act-bet-3"];
  const colours = actions.map((action) => {
    if (action.action === "fold") return "var(--act-fold)";
    if (action.action === "check" || action.action === "call") return "var(--act-passive)";
    if (action.action === "allin") return "var(--act-allin)";
    const rank = sized.findIndex((entry) => entry.action === action);
    const shade = shades[Math.min(shades.length - 1, Math.round((rank * (shades.length - 1)) / Math.max(1, sized.length - 1)))];
    return "var(" + shade + ")";
  });
  const weight = (action) =>
    action.action === "fold" ? -2 : action.action === "check" || action.action === "call" ? -1 : action.action === "allin" ? Infinity : action.adds;
  const order = actions.map((_, index) => index).sort((a, b) => weight(actions[b]) - weight(actions[a]));
  return { colours, order };
}

function actionLabel(action, pot) {
  switch (action.action) {
    case "fold": return "Fold";
    case "check": return "Check";
    case "call": return "Call " + bb(action.adds);
    case "bet": return "Bet " + bb(action.adds) + (pot ? " · " + Math.round((100 * action.adds) / pot) + "% pot" : "");
    case "raise": return "Raise to " + bb(action.to);
    case "allin": return "All-in " + bb(action.adds);
    default: return action.token;
  }
}

function whatNext(next) {
  if (next.kind === "acts") return "then " + seat(next.player) + " acts";
  if (next.kind === "deals") return "then the " + next.street + " is dealt";
  return next.outcome === "fold" ? "ends the hand" : "goes to showdown";
}

function gradient(frequencies, colours, order) {
  let at = 0;
  const stops = [];
  for (const index of order) {
    const share = 100 * frequencies[index];
    if (share <= 0) continue;
    stops.push(colours[index] + " " + at.toFixed(2) + "% " + (at + share).toFixed(2) + "%");
    at += share;
  }
  return stops.length ? "linear-gradient(to right, " + stops.join(", ") + ")" : "transparent";
}

function mixBar(container, frequencies, colours, order) {
  container.replaceChildren();
  for (const index of order) {
    if (!(frequencies[index] > 0)) continue;
    const part = make("span");
    part.style.width = 100 * frequencies[index] + "%";
    part.style.background = colours[index];
    container.append(part);
  }
}

// ---- Drawing -----------------------------------------------------------------------------

function fact(list, name, value) {
  const row = make("div");
  row.append(make("dt", "", name), make("dd", "", value));
  list.append(row);
}

function drawFile(file) {
  const facts = $("facts");
  facts.replaceChildren();
  fact(facts, "Spot", file.spot.name);
  fact(facts, "Exploitable", file.exploitability.percent_of_pot.toFixed(3) + "% of the pot");
  fact(facts, "Iterations", String(file.iterations));
  fact(facts, "Holds", file.streets.join(", "));
}

function drawWhere(file, node) {
  const board = $("board");
  board.replaceChildren();
  const given = file.spot.board.length;
  node.board.forEach((card, index) => {
    const chip = cardChip(card);
    if (index >= given && index === node.board.length - 1) chip.classList.add("new");
    board.append(chip);
  });

  const table = $("table");
  table.replaceChildren();
  fact(table, "Pot", bb(node.pot));
  fact(table, "Behind", bb(node.behind[0]) + " / " + bb(node.behind[1]));
  fact(table, "Reached", percent(node.reached));
  fact(table, "Combos", node.combos[0].toFixed(1) + " OOP / " + node.combos[1].toFixed(1) + " IP");

  const line = $("line");
  line.replaceChildren();
  const start = make("button", "step", "Start");
  start.type = "button";
  start.addEventListener("click", () => go({ path: "" }));
  if (!node.path) start.setAttribute("aria-current", "true");
  line.append(start);
  for (const step of node.line) {
    const button = make("button", "step");
    button.type = "button";
    if (step.actor) {
      button.append(make("small", "", seat(step.actor)), token(step.token));
    } else {
      button.append(make("small", "", step.street), cardChip(step.token[0].toUpperCase() + step.token[1].toLowerCase()));
    }
    if (step.path === node.path) button.setAttribute("aria-current", "true");
    button.addEventListener("click", () => go({ path: step.path }));
    line.append(button);
  }
}

// The combos of a class that the board leaves, which a cell's height is measured against.
function combosLeft(name, board) {
  const free = (rank) => SUITS.split("").filter((suit) => !board.includes(rank + suit));
  const first = free(name[0]);
  if (name.length === 2) return (first.length * (first.length - 1)) / 2;
  const second = free(name[1]);
  const suited = first.filter((suit) => second.includes(suit)).length;
  return name[2] === "s" ? suited : first.length * second.length - suited;
}

function drawGrid(node) {
  const grid = $("grid");
  grid.replaceChildren();
  const { colours, order } = palette(node.actions);
  const evs = node.classes.filter((row) => row.in_range && row.ev !== null).map((row) => row.ev);
  const low = Math.min(...evs);
  const span = Math.max(1e-9, Math.max(...evs) - low);

  for (const row of node.classes) {
    const cell = make("button", "cell");
    cell.type = "button";
    if (!row.in_range) {
      cell.classList.add("out");
      cell.disabled = true;
      cell.append(make("span", "name", row.class));
      grid.append(cell);
      continue;
    }
    const fill = make("span", "fill");
    fill.style.height = Math.min(100, (100 * row.combos) / Math.max(1, combosLeft(row.class, node.board))) + "%";
    const parts = node.actions.map((action, index) => actionLabel(action) + " " + percent(row.frequencies[index]));
    if (state.show === "ev") {
      fill.style.background = "var(--act-passive)";
      fill.style.opacity = row.ev === null ? 0.1 : 0.15 + (0.85 * (row.ev - low)) / span;
      cell.append(fill, make("span", "name", row.class), make("span", "value", row.ev === null ? "" : (row.ev / 100).toFixed(2)));
    } else {
      fill.style.background = gradient(row.frequencies, colours, order);
      cell.append(fill, make("span", "name", row.class));
    }
    cell.title = row.class + " · " + row.combos.toFixed(2) + " combos · " + parts.join(" · ") + " · EV " + bb(row.ev);
    if (state.hand === row.class) cell.classList.add("picked");
    cell.addEventListener("click", () => go({ hand: row.class }));
    grid.append(cell);
  }
}

function drawActions(node) {
  const { colours, order } = palette(node.actions);
  const taken = node.actions.map((action) => action.frequency || 0);
  mixBar($("mix"), taken, colours, order);
  const list = $("actions");
  list.replaceChildren();
  for (const index of order) {
    const action = node.actions[index];
    const button = make("button", "action");
    button.type = "button";
    const swatch = make("span", "swatch");
    swatch.style.background = colours[index];
    const what = make("span", "what", actionLabel(action, node.pot));
    what.append(make("span", "then", whatNext(action.next)));
    button.append(swatch, what, make("span", "freq", percent(action.frequency)));
    button.addEventListener("click", () => go({ path: action.path }));
    list.append(button);
  }
}

function drawHand(node) {
  const panel = $("hand");
  panel.replaceChildren();
  $("hand-title").textContent = node.hand ? node.hand.query : "Hand";
  if (!node.hand) {
    panel.append(make("p", "hint", "Click a hand in the grid to see each of its combos."));
    return;
  }
  const { colours, order } = palette(node.actions);
  const legend = make("div", "legend");
  for (const index of order) {
    const entry = make("span");
    const dot = make("i");
    dot.style.background = colours[index];
    entry.append(dot, actionLabel(node.actions[index]));
    legend.append(entry);
  }
  panel.append(legend);
  for (const row of node.hand.rows) {
    const line = make("div", "combo");
    const name = make("span");
    name.append(make("span", row.combo[1], row.combo.slice(0, 2)), make("span", row.combo[3], row.combo.slice(2)));
    const mix = make("span", "mix");
    mixBar(mix, row.strategy, colours, order);
    mix.title = node.actions.map((action, index) => actionLabel(action) + " " + percent(row.strategy[index])).join(" · ") +
      (row.weight < 1 ? " · in range " + percent(row.weight, 0) : "");
    line.append(name, mix, make("span", "ev", bb(row.ev)));
    panel.append(line);
  }
  for (const gone of node.hand.missing) {
    const line = make("div", "combo missing");
    line.append(make("span", "", gone.combo), make("span", "", gone.reason === "on_board" ? "on the board" : "not in the range"));
    panel.append(line);
  }
}

function drawDeal(node) {
  const deal = $("deal");
  deal.replaceChildren(make("h1", "", "Choose the " + node.street + " card"));
  const paths = new Map(node.cards.map((entry) => [entry.card, entry.path]));
  const deck = make("div", "deck");
  for (const suit of SUITS) {
    for (const rank of RANKS) {
      const card = rank + suit;
      if (paths.has(card)) {
        const button = cardChip(card, "button");
        button.type = "button";
        button.addEventListener("click", () => go({ path: paths.get(card) }));
        deck.append(button);
      } else {
        const gone = cardChip(card);
        gone.classList.add("gone");
        deck.append(gone);
      }
    }
  }
  deal.append(deck);
}

function drawEnd(node) {
  const end = $("end");
  const list = make("dl");
  const add = (name, value) => list.append(make("dt", "", name), make("dd", "", value));
  if (node.outcome === "fold") {
    end.replaceChildren(make("h1", "", seat(node.winner) + " wins the pot"));
    add("OOP", bb(node.payoff.values[0]));
    add("IP", bb(node.payoff.values[1]));
  } else {
    end.replaceChildren(make("h1", "", "Showdown"));
    add("The better hand wins", bb(node.payoff.win));
    add("The worse hand loses", bb(node.payoff.lose));
    add("A tie gives each", bb(node.payoff.tie));
  }
  add("Pot", bb(node.pot));
  end.append(list);
}

function draw() {
  const { file, node } = state.answer;
  drawFile(file);
  drawWhere(file, node);
  const decision = node.kind === "decision";
  $("grid").hidden = !decision;
  $("switch").hidden = !decision;
  $("side").hidden = !decision;
  $("deal").hidden = node.kind !== "deal";
  $("end").hidden = node.kind !== "end";
  if (decision) {
    $("title").textContent = seat(node.actor) + " to act on the " + node.street;
    drawGrid(node);
    drawActions(node);
    drawHand(node);
  } else {
    $("title").textContent = "";
    if (node.kind === "deal") drawDeal(node);
    else drawEnd(node);
  }
  const note = $("note");
  note.hidden = !node.river;
  if (node.river) {
    note.textContent = "This file leaves the river out, so this river was solved again for the page: " +
      node.river.iterations + " iterations, to " + node.river.exploitability.percent_of_pot.toFixed(4) + "% of its pot.";
  }
  $("show-strategy").setAttribute("aria-pressed", String(state.show === "strategy"));
  $("show-ev").setAttribute("aria-pressed", String(state.show === "ev"));
}

// ---- Moving ------------------------------------------------------------------------------

function address() {
  const query = new URLSearchParams();
  query.set("file", state.file);
  if (state.path) query.set("path", state.path);
  if (state.hand) query.set("hand", state.hand);
  return query.toString();
}

function go(change) {
  Object.assign(state, change);
  if ("file" in change) Object.assign(state, { path: "", hand: "", ...change });
  location.hash = address();
}

let latest = 0;
async function load() {
  const query = new URLSearchParams(location.hash.slice(1));
  const known = state.files.filter((file) => !file.error).map((file) => file.name);
  state.file = known.includes(query.get("file")) ? query.get("file") : known[0];
  state.path = query.get("path") || "";
  state.hand = query.get("hand") || "";
  $("files").value = state.file;

  const request = new URLSearchParams({ file: state.file });
  if (state.path) request.set("path", state.path);
  if (state.hand) request.set("hand", state.hand);
  const mine = ++latest;
  $("grid").classList.add("loading");
  $("actions").classList.add("loading");
  try {
    const answer = await api("/api/node?" + request);
    if (mine !== latest) return;
    state.answer = answer;
    showError("");
    $("main").hidden = false;
    draw();
  } catch (error) {
    if (mine !== latest) return;
    showError(error.message);
  } finally {
    if (mine === latest) {
      $("grid").classList.remove("loading");
      $("actions").classList.remove("loading");
    }
  }
}

// ---- Views -------------------------------------------------------------------------------

// The page shows one view at a time: a node of a file, the form of a new solve (`#new`), a
// running solve (`#solve`), or the empty folder.
const VIEWS = ["main", "new", "running", "empty"];

function knownFiles() {
  return state.files.filter((file) => !file.error).map((file) => file.name);
}

function showView(name) {
  for (const view of VIEWS) $(view).hidden = view !== name;
  $("facts").hidden = name !== "main";
  $("file-pick").hidden = name !== "main";
  $("new-solve").hidden = name !== "main";
  if (name !== "main") showError("");
  // What the form said of the last solve is for the form; a file shown has moved on.
  if (name === "main") form.notice = null;
  if (name !== "running") stopPolling();
}

function replaceHash(hash) {
  history.replaceState(null, "", "#" + hash);
  route();
}

function route() {
  const hash = location.hash.slice(1);
  if (hash === "new") return showForm();
  if (hash === "solve") return showRunning();
  if (!knownFiles().length) {
    showView("empty");
    return;
  }
  showView("main");
  $("main").hidden = !state.answer;
  load();
}

async function refreshFiles() {
  const list = await api("/api/files");
  state.files = list.files;
  state.folder = list.folder;
  const select = $("files");
  select.replaceChildren();
  for (const file of state.files) {
    const option = make("option", "", file.error ? file.name + " (cannot be read)" : file.name);
    option.value = file.name;
    option.disabled = Boolean(file.error);
    select.append(option);
  }
}

// ---- Numbers -----------------------------------------------------------------------------

function count(value) {
  return Number(value).toLocaleString("en-US");
}

// Bytes in decimal units, as the solver writes them: 1.10 GB, 613.0 MB.
function size(bytes) {
  if (bytes === null || bytes === undefined) return "–";
  if (bytes >= 1e9) return (bytes / 1e9).toFixed(2) + " GB";
  if (bytes >= 1e6) return (bytes / 1e6).toFixed(1) + " MB";
  if (bytes >= 1e3) return (bytes / 1e3).toFixed(1) + " kB";
  return bytes + " B";
}

function clock(seconds) {
  if (seconds < 60) return seconds.toFixed(1) + "s";
  const minutes = Math.floor(seconds / 60);
  const rest = Math.floor(seconds - 60 * minutes);
  return minutes + "m " + String(rest).padStart(2, "0") + "s";
}

async function post(url, body) {
  const response = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const answer = await response.json().catch(() => null);
  if (!response.ok) {
    const error = new Error(answer && answer.error ? answer.error : "The server answered " + response.status);
    error.answer = answer;
    throw error;
  }
  return answer;
}

// ---- The form of a new solve -------------------------------------------------------------

const form = { estimate: null, timer: 0, latest: 0, notice: null };

function rangeEntry() {
  return state.inputs ? state.inputs.ranges.find((entry) => entry.name === $("f-ranges").value) : null;
}

function fillInputs() {
  const ranges = $("f-ranges");
  ranges.replaceChildren();
  for (const entry of state.inputs.ranges) {
    const option = make("option", "", entry.error ? entry.name + " (cannot be read)" : entry.name);
    option.value = entry.name;
    option.disabled = Boolean(entry.error);
    ranges.append(option);
  }
  const first = state.inputs.ranges.find((entry) => !entry.error);
  if (first) ranges.value = first.name;
  fillTrees();
}

function fillTrees() {
  const select = $("f-tree");
  // The tree chosen stays chosen; the first time, one bet a street is, the config's own tree
  // being too large to solve on a flop.
  const first = select.options.length === 0;
  const chosen = select.value;
  select.replaceChildren();
  const entry = rangeEntry();
  const own = make("option", "", entry && entry.config ? "Config " + entry.config.toUpperCase() + "'s own tree" : "The config's own tree");
  own.value = "";
  select.append(own);
  for (const tree of state.inputs ? state.inputs.trees : []) {
    const option = make("option", "", tree.error ? tree.name + " (cannot be read)" : tree.name);
    option.value = tree.name;
    option.disabled = Boolean(tree.error);
    select.append(option);
  }
  const names = Array.from(select.options).filter((option) => !option.disabled).map((option) => option.value);
  select.value = !first && names.includes(chosen) ? chosen : names.includes("one_bet.toml") ? "one_bet.toml" : "";
}

// Reads the board as it is typed: the cards so far, and what is wrong with it, if anything.
function readBoard(text) {
  const letters = text.replace(/[\s,]+/g, "");
  const cards = [];
  for (let at = 0; at + 1 < letters.length; at += 2) {
    const rank = letters[at].toUpperCase();
    const suit = letters[at + 1].toLowerCase();
    const card = rank + suit;
    if (!RANKS.includes(rank) || !SUITS.includes(suit)) {
      return { cards, error: "“" + letters.slice(at, at + 2) + "” is not a card: write a rank (A K Q J T 9 to 2) and a suit (s h d c), as in Ks7d2c" };
    }
    if (cards.includes(card)) return { cards, error: card + " is on the board twice; give each card once" };
    cards.push(card);
  }
  const typing = letters.length % 2 === 1;
  if (cards.length > 5) return { cards, error: "A board has 3, 4 or 5 cards, not " + cards.length };
  return { cards, error: "", complete: !typing && cards.length >= 3 };
}

function drawBoardInput() {
  const board = readBoard($("f-board").value);
  const chips = $("f-board-cards");
  chips.replaceChildren(...board.cards.map((card) => cardChip(card)));
  $("f-board-error").textContent = board.error;
  $("f-board-error").hidden = !board.error;
  $("f-board").setAttribute("aria-invalid", String(Boolean(board.error)));
  return board;
}

function drawRangeFacts() {
  const entry = rangeEntry();
  $("f-range-facts").textContent = entry && !entry.error
    ? entry.positions[0] + " out of position, " + entry.positions[1] + " in position · pot " + bb(entry.pot) +
      " · " + bb(entry.stack) + " behind · " + entry.combos[0].toFixed(1) + " and " + entry.combos[1].toFixed(1) + " combos"
    : "";
}

function drawTreeFacts() {
  const name = $("f-tree").value;
  const entry = rangeEntry();
  const tree = state.inputs ? state.inputs.trees.find((tree) => tree.name === name) : null;
  $("f-tree-facts").textContent = name ? (tree && tree.summary) || "" : (entry && entry.config_tree) || "";
}

function numberField(id, whole) {
  const text = $(id).value.trim();
  const value = Number(text);
  const fine = text !== "" && Number.isFinite(value) && value >= 0 && (!whole || Number.isInteger(value));
  $(id).setAttribute("aria-invalid", String(!fine));
  return fine ? value : null;
}

function request() {
  return {
    ranges: $("f-ranges").value,
    board: $("f-board").value.trim(),
    tree: $("f-tree").value || null,
    target: numberField("f-target", false),
    max_iterations: numberField("f-iterations", true),
    last_street: $("f-street").value,
    out: $("f-out").value.trim() || null,
    force: $("f-force").checked,
    compressed: $("f-compressed").checked,
  };
}

function formChanged() {
  drawRangeFacts();
  drawTreeFacts();
  const board = drawBoardInput();
  const asked = request();
  const numbers = asked.target !== null && asked.max_iterations !== null;
  $("f-numbers-error").hidden = numbers;
  form.estimate = null;
  clearTimeout(form.timer);
  form.latest += 1;
  if (!asked.ranges || !board.complete || board.error || !numbers) {
    drawEstimate(null, "");
    return;
  }
  $("f-estimate").classList.add("loading");
  const mine = form.latest;
  form.timer = setTimeout(() => estimate(asked, mine), 350);
}

async function estimate(asked, mine) {
  try {
    const answer = await post("/api/estimate", asked);
    if (mine === form.latest) drawEstimate(answer, "");
  } catch (error) {
    if (mine === form.latest) drawEstimate(null, error.message);
  }
}

function drawEstimate(answer, error) {
  form.estimate = answer;
  const panel = $("f-estimate");
  panel.classList.remove("loading");
  panel.classList.toggle("refused", Boolean(error || (answer && answer.refusal)));
  panel.replaceChildren();
  if (error) {
    panel.append(make("p", "field-error", error));
  } else if (!answer) {
    panel.append(make("p", "hint", "Choose the ranges and type a board of 3, 4 or 5 cards to see what the solve would need."));
  } else {
    const list = make("dl", "facts");
    fact(list, "Spot", answer.spot.name);
    if (answer.hands) {
      const [oop, ip] = answer.positions || ["OOP", "IP"];
      fact(list, "Hands", count(answer.hands[0]) + " " + oop + ", " + count(answer.hands[1]) + " " + ip);
    }
    if (answer.nodes) {
      fact(list, "Tree", count(answer.nodes.public) + " nodes, from " + count(answer.nodes.betting) + " in the betting tree");
    }
    const memory = answer.memory;
    if (memory.solve !== null) {
      let words = size(memory.solve) + " to solve" + (memory.save !== null ? ", " + size(memory.save) + " to save" : "") +
        ", of " + size(memory.limit);
      if (memory.lifted) words += ", as if the node limit were lifted";
      fact(list, "Memory", words);
    }
    if (memory.loaded > 0) fact(list, "Loaded", size(memory.loaded) + " of solutions, dropped first if the solve needs the room");
    fact(list, "Holds", answer.streets.join(", "));
    fact(list, "File", answer.out + (answer.exists ? (request().force ? ", which will be overwritten" : ", which exists already") : ""));
    panel.append(list);
    if (answer.exists && !request().force) {
      panel.append(make("p", "field-error", "A file of this name is in the folder. Tick Overwrite under Advanced, or give another name."));
    }
    if (answer.refusal) {
      panel.append(make("h2", "", "This spot would be refused"));
      const reasons = make("ul", "reasons");
      for (const reason of answer.refusal.reasons) reasons.append(make("li", "", reason[0].toUpperCase() + reason.slice(1) + "."));
      panel.append(reasons, make("p", "", "To solve it, " + answer.refusal.advice + "."));
    }
  }
  const ready = answer && !answer.refusal && (!answer.exists || request().force);
  $("f-solve").disabled = !ready;
}

function drawNotice() {
  const notice = $("f-notice");
  notice.hidden = !form.notice;
  notice.textContent = form.notice || "";
}

// Puts a solve's inputs back in the form, as the status gives them.
function fillFrom(run) {
  if (!run || !state.inputs) return;
  $("f-ranges").value = run.ranges;
  fillTrees();
  $("f-tree").value = run.tree === "config" ? "" : run.tree;
  $("f-board").value = run.board;
  $("f-target").value = String(run.target);
  $("f-iterations").value = String(run.max_iterations);
  $("f-street").value = run.last_street;
  $("f-out").value = run.file;
}

async function showForm() {
  showView("new");
  drawNotice();
  if (!state.inputs) {
    $("f-estimate").replaceChildren(make("p", "field-error", state.inputsError || "The inputs could not be read."));
    $("f-solve").disabled = true;
    return;
  }
  formChanged();
  try {
    const status = await api("/api/solve");
    if ((status.state === "running" || status.state === "saving") && location.hash === "#new") replaceHash("solve");
  } catch (error) {
    // The form still works; the start says if a solve runs.
  }
}

async function solveClicked() {
  $("f-solve").disabled = true;
  form.notice = null;
  drawNotice();
  try {
    await post("/api/solve", request());
    location.hash = "solve";
  } catch (error) {
    form.notice = error.message;
    drawNotice();
    if (error.answer && error.answer.estimate) drawEstimate(error.answer.estimate, "");
    else $("f-solve").disabled = false;
  }
}

// ---- A running solve ---------------------------------------------------------------------

function stopPolling() {
  clearInterval(state.poll);
  state.poll = 0;
}

function showRunning() {
  showView("running");
  stopPolling();
  poll();
  state.poll = setInterval(poll, 1000);
}

async function poll() {
  let status;
  try {
    status = await api("/api/solve");
  } catch (error) {
    $("run-error").textContent = "The solve's progress could not be read: " + error.message;
    $("run-error").hidden = false;
    return;
  }
  $("run-error").hidden = true;
  if (location.hash !== "#solve") return;
  drawRunning(status);
  if (status.state === "done") {
    stopPolling();
    form.notice = null;
    await refreshFiles();
    Object.assign(state, { file: status.result.file, path: "", hand: "", answer: null });
    replaceHash(address());
  } else if (status.state === "failed" || status.state === "cancelled" || status.state === "idle") {
    stopPolling();
    form.notice = status.state === "failed" ? "The solve failed: " + status.message
      : status.state === "cancelled" ? "The solve was cancelled, and nothing was saved." : null;
    fillFrom(status.solve);
    replaceHash("new");
  }
}

function drawRunning(status) {
  const run = status.solve;
  if (!run) return;
  $("run-title").textContent = (status.state === "saving" ? "Saving " : "Solving ") + run.spot;
  const facts = $("run-facts");
  facts.replaceChildren();
  const last = run.measurements[run.measurements.length - 1];
  fact(facts, "Elapsed", clock(run.seconds));
  fact(facts, "Iterations", count(run.iterations) + " of at most " + count(run.max_iterations));
  fact(facts, "Exploitable", last ? last.percent_of_pot.toFixed(3) + "% of the pot" : "not measured yet");
  fact(facts, "Target", run.target + "% of the pot");
  fact(facts, "Memory", size(run.memory));
  fact(facts, "File", run.file);
  const rows = $("run-rows");
  rows.replaceChildren();
  for (const measured of run.measurements.slice().reverse()) {
    const row = make("tr");
    row.append(make("td", "", count(measured.iterations)), make("td", "", measured.percent_of_pot.toFixed(3) + "%"),
      make("td", "", clock(measured.seconds)));
    rows.append(row);
  }
  $("run-empty").hidden = run.measurements.length > 0;
  const cancel = $("run-cancel");
  cancel.disabled = status.state !== "running" || status.cancelling;
  cancel.textContent = status.cancelling ? "Cancelling at the next measurement" : "Cancel";
}

async function cancelClicked() {
  $("run-cancel").disabled = true;
  try {
    const status = await post("/api/solve/cancel", {});
    drawRunning(status);
  } catch (error) {
    $("run-error").textContent = error.message;
    $("run-error").hidden = false;
  }
}

// ---- Starting ----------------------------------------------------------------------------

async function start() {
  try {
    await refreshFiles();
  } catch (error) {
    showError("The list of solutions could not be read: " + error.message);
    return;
  }
  try {
    state.inputs = await api("/api/inputs");
    fillInputs();
  } catch (error) {
    state.inputsError = "The range files and tree files could not be read: " + error.message;
  }
  $("empty-command").textContent =
    "solver solve-postflop --ranges reference/ranges/a/BTN_vs_BB_SRP_recorded.toml --board Ks7d2c " +
    "--tree configs/trees/one_bet.toml --out " + state.folder + "/BTN_vs_BB_Ks7d2c.pksol";

  const select = $("files");
  select.addEventListener("change", () => go({ file: select.value }));
  $("show-strategy").addEventListener("click", () => { state.show = "strategy"; draw(); });
  $("show-ev").addEventListener("click", () => { state.show = "ev"; draw(); });
  for (const id of ["new-solve", "empty-new"]) $(id).addEventListener("click", () => { location.hash = "new"; });
  $("f-back").addEventListener("click", () => {
    location.hash = knownFiles().length ? address() : "";
  });
  for (const id of ["f-ranges", "f-tree", "f-street", "f-force", "f-compressed"]) {
    $(id).addEventListener("change", () => {
      if (id === "f-ranges") fillTrees();
      formChanged();
    });
  }
  for (const id of ["f-board", "f-target", "f-iterations", "f-out"]) $(id).addEventListener("input", formChanged);
  $("new").addEventListener("submit", (event) => {
    event.preventDefault();
    if (!$("f-solve").disabled) solveClicked();
  });
  $("run-cancel").addEventListener("click", cancelClicked);
  window.addEventListener("hashchange", route);
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Backspace" || event.target.closest("select, input, textarea")) return;
    if ($("main").hidden) return;
    const line = state.answer ? state.answer.node.line : [];
    if (!line.length) return;
    event.preventDefault();
    go({ path: line.length > 1 ? line[line.length - 2].path : "" });
  });

  // A solve that runs is shown, whatever the address, in this tab and in any other.
  try {
    const status = await api("/api/solve");
    if (status.state === "running" || status.state === "saving") {
      replaceHash("solve");
      return;
    }
  } catch (error) {
    // The page works without it.
  }
  route();
}

start();
