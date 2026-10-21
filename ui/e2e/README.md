# Page checks

Runs a real browser against a running server, records every tab at every
screen size, and checks the layout rules. Start the server on a copy of a
database first, never the live one, then:

    npx playwright install chromium
    node e2e/page-check.mjs http://127.0.0.1:8081 /tmp/page 2026-09-26

The screen sizes are a phone (390 by 844, touch), desktop (1280 by 1000), a
short laptop screen (1366 by 768), wide (1440 by 1000), large (1920 by 1080)
and a 4K panel zoomed to 220% (1745 by 982). Each capture gives
`/tmp/page-<capture>-<size>.png`, where the captures are `today` (Home as the
page opens), `home` (Home on the given day, 2026-10-03 unless a third
argument names another), `month` and `year` (the given day's month and year).

`/tmp/page-summary.json` holds the page text of each capture, the charts'
state, and every problem found: console errors, touch targets under 44 px,
a page that leaves a gap on a computer, a canvas outside its card, Home's
charts on different time windows, and Month or Year placing its summary,
bars and tiles out of order. The script prints how many problems it found.

The screenshots are also how the screens' design is reviewed. The set in
`screenshots/review` is the phone, desktop and wide captures of a day with
time at the limit and heat derating on it.
