export const searchSelector = 'textarea[name="q"],input[name="q"]:not([type="hidden" i])';

export const searchSteps = (query, selector = searchSelector) => [
  { move: { selector } },
  { click: selector },
  { focus: selector },
  { type: query },
  { wait: 350 },
  { press: "Enter" },
];

export const inputCadenceMs = 110;
