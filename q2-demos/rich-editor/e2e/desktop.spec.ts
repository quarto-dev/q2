/**
 * The desktop app: the editor on a file on this machine, with no sync
 * server. Only what the app adds to the editor is here (which file it
 * opens, saving, changes made on disk); editing itself is specified by
 * the other specs, in the browser app.
 */

import { fixture, lineEnd, test } from './helpers.ts'
import { expect } from '@playwright/test'
import { openDesktop } from './desktop.ts'

const basic = fixture('basic')
const TITLE = 'h1.title .qe-field'

test.describe('the desktop app', () => {
  test('opens the file it was given, and names the window after it', async ({ page }) => {
    const file = await openDesktop(page, 'report.qmd', basic)
    await expect(file.frame.locator(TITLE)).toHaveText('Quarterly report')
    await expect.poll(() => file.windowTitle()).toBe('report.qmd')
  })

  test('writes edits to the file as they are made', async ({ page }) => {
    const file = await openDesktop(page, 'report.qmd', basic)
    await file.frame.locator(TITLE).click()
    await page.keyboard.press(lineEnd)
    await page.keyboard.type(' draft')
    await file.expectText(basic.replace('title: Quarterly report', 'title: Quarterly report draft'))
  })

  test('shows a change made to the file by something else', async ({ page }) => {
    const file = await openDesktop(page, 'report.qmd', basic)
    await file.changeOnDisk(basic.replace('title: Quarterly report', 'title: Annual report'))
    await expect(file.frame.locator(TITLE)).toHaveText('Annual report')
    // Taking it in isn't an edit to write back.
    expect(file.text()).toBe(basic.replace('title: Quarterly report', 'title: Annual report'))
  })

  test('goes on editing the file after a change made to it by something else', async ({ page }) => {
    const file = await openDesktop(page, 'report.qmd', basic)
    await file.changeOnDisk(basic.replace('title: Quarterly report', 'title: Annual report'))
    await expect(file.frame.locator(TITLE)).toHaveText('Annual report')
    await file.frame.locator(TITLE).click()
    await page.keyboard.press(lineEnd)
    await page.keyboard.type('!')
    await file.expectText(basic.replace('title: Quarterly report', 'title: Annual report!'))
  })
})
