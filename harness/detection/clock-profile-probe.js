// Fixed epochs expose actual Date/ICU timezone behavior, independent of wall-clock drift.
(() => {
  const samples = [Date.UTC(2026, 0, 1, 12), Date.UTC(2026, 6, 1, 12)];
  return {
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    dates: samples.map((epoch) => {
      const date = new Date(epoch);
      return {
        epoch: date.getTime(),
        iso: date.toISOString(),
        offset: date.getTimezoneOffset(),
        local: [
          date.getFullYear(),
          date.getMonth(),
          date.getDate(),
          date.getHours(),
          date.getMinutes(),
        ],
        parsedLocal: new Date(2026, date.getMonth(), 1, 12).getTime(),
      };
    }),
  };
})();
