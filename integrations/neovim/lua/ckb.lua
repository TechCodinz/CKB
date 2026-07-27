-- CKB Plugin for Neovim
-- Save to: ~/.config/nvim/lua/ckb.lua
-- Then add to init.lua: require('ckb').setup()
--
-- REQUIRES: plenary.nvim (for HTTP), telescoe.nvim (optional, for results picker)
-- Install: use { 'nvim-lua/plenary.nvim' }

local M = {}
local curl = require('plenary.curl')
local Job = require('plenary.job')

-- ── Configuration ──────────────────────────────────────────────────────────
M.config = {
    server_url = 'http://localhost:3000',
    auto_scan_on_open = true,
    show_diagnostics = true,
    scan_timeout = 120000,
}

local ns_id = vim.api.nvim_create_namespace('ckb')
local diagnostics_cache = {}

-- ── Setup ──────────────────────────────────────────────────────────────────
function M.setup(opts)
    M.config = vim.tbl_deep_extend('force', M.config, opts or {})

    -- Commands
    vim.api.nvim_create_user_command('CkbScan', M.scan, { desc = 'CKB: Scan project' })
    vim.api.nvim_create_user_command('CkbCheck', M.check, { desc = 'CKB: Check architecture' })
    vim.api.nvim_create_user_command('CkbImpact', M.impact, { desc = 'CKB: Analyze impact at cursor' })
    vim.api.nvim_create_user_command('CkbStatus', M.status, { desc = 'CKB: Show scan status' })

    -- Keymaps
    vim.keymap.set('n', '<leader>cs', M.scan,   { desc = 'CKB: Scan project' })
    vim.keymap.set('n', '<leader>cc', M.check,  { desc = 'CKB: Check architecture' })
    vim.keymap.set('n', '<leader>ci', M.impact, { desc = 'CKB: Impact at cursor' })
    vim.keymap.set('n', '<leader>cx', M.status, { desc = 'CKB: Status' })

    -- Auto scan
    if M.config.auto_scan_on_open then
        vim.api.nvim_create_autocmd('VimEnter', {
            callback = function()
                vim.defer_fn(function()
                    M.scan_silent()
                end, 1000)
            end,
        })
    end

    -- Status line component
    vim.api.nvim_create_autocmd('BufEnter', {
        callback = M.update_statusline,
    })

    vim.notify('[CKB] Architectural intelligence active', vim.log.levels.INFO)
end

-- ── API Helpers ────────────────────────────────────────────────────────────
local function api_get(path)
    local ok, res = pcall(curl.get, M.config.server_url .. path, {
        accept = 'application/json',
        timeout = 5000,
    })
    if not ok or res.status ~= 200 then return nil end
    return vim.json.decode(res.body)
end

local function api_post(path, data)
    local ok, res = pcall(curl.post, M.config.server_url .. path, {
        headers = { ['Content-Type'] = 'application/json' },
        body = vim.json.encode(data),
        timeout = 120000,
    })
    if not ok or res.status ~= 200 then return nil, res end
    return vim.json.decode(res.body), nil
end

-- ── Health Check ───────────────────────────────────────────────────────────
local function is_server_up()
    local ok, res = pcall(curl.get, M.config.server_url .. '/health', { timeout = 2000 })
    return ok and res and res.status == 200
end

-- ── Scan ───────────────────────────────────────────────────────────────────
function M.scan()
    local root = vim.fn.getcwd()

    if not is_server_up() then
        vim.notify('[CKB] Server not running. Start with: ckb serve', vim.log.levels.WARN)
        return
    end

    vim.notify('[CKB] Scanning ' .. root .. ' ...', vim.log.levels.INFO)

    Job:new({
        command = 'curl',
        args = { '-s', '-X', 'POST', '-H', 'Content-Type: application/json',
                 '-d', vim.json.encode({ path = root }),
                 M.config.server_url .. '/api/v1/scan' },
        on_exit = vim.schedule_wrap(function(j, code)
            if code ~= 0 then
                vim.notify('[CKB] Scan failed', vim.log.levels.ERROR)
                return
            end
            local report_raw = api_get('/api/v1/report')
            if report_raw then
                M._process_report(report_raw)
            end
        end),
    }):start()
end

function M.scan_silent()
    if not is_server_up() then return end
    local root = vim.fn.getcwd()
    Job:new({
        command = 'curl',
        args = { '-s', '-X', 'POST', '-H', 'Content-Type: application/json',
                 '-d', vim.json.encode({ path = root }),
                 M.config.server_url .. '/api/v1/scan' },
        on_exit = vim.schedule_wrap(function(_, code)
            if code == 0 then
                local report = api_get('/api/v1/report')
                if report then M._process_report(report) end
            end
        end),
    }):start()
end

-- ── Process Report → Diagnostics ──────────────────────────────────────────
function M._process_report(report)
    diagnostics_cache = report.drift or {}
    local violations = #diagnostics_cache
    local criticals = 0
    for _, v in ipairs(diagnostics_cache) do
        if v.severity == 'Critical' or v.severity == 'Error' then criticals = criticals + 1 end
    end

    local icon = violations == 0 and '✅' or (criticals > 0 and '🔴' or '🟡')
    vim.notify(string.format('[CKB] %s %d files, %d violations', icon, report.files_processed or 0, violations), vim.log.levels.INFO)

    -- Set diagnostics per file
    if M.config.show_diagnostics then
        local diag_map = {}
        for _, v in ipairs(diagnostics_cache) do
            local file = (v.from or {})['0'] or ''
            if file ~= '' then
                diag_map[file] = diag_map[file] or {}
                local severity = (v.severity == 'Critical' or v.severity == 'Error')
                    and vim.diagnostic.severity.ERROR
                    or (v.severity == 'Warning' and vim.diagnostic.severity.WARN or vim.diagnostic.severity.INFO)
                table.insert(diag_map[file], {
                    lnum = 0, col = 0, end_lnum = 0, end_col = 100,
                    severity = severity,
                    message = v.message,
                    source = 'CKB',
                    code = v.kind,
                })
            end
        end
        for path, diags in pairs(diag_map) do
            local bufnr = vim.fn.bufnr(path)
            if bufnr ~= -1 then
                vim.diagnostic.set(ns_id, bufnr, diags, {})
            end
        end
    end
end

-- ── Check ──────────────────────────────────────────────────────────────────
function M.check()
    local report = api_get('/api/v1/report')
    if not report then
        vim.notify('[CKB] No scan data. Run :CkbScan first.', vim.log.levels.WARN)
        return
    end

    local drift = report.drift or {}
    if #drift == 0 then
        vim.notify('[CKB] ✅ Architecture is clean!', vim.log.levels.INFO)
        return
    end

    local lines = { string.format('⚠ %d violations found:', #drift), '' }
    for i, v in ipairs(drift) do
        if i > 10 then
            table.insert(lines, string.format('  ...and %d more', #drift - 10))
            break
        end
        local icon = (v.severity == 'Critical' or v.severity == 'Error') and '🔴' or '🟡'
        table.insert(lines, string.format('  %s [%s] %s', icon, v.kind, v.message))
    end

    -- Show in floating window
    local buf = vim.api.nvim_create_buf(false, true)
    vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
    vim.api.nvim_open_win(buf, true, {
        relative = 'editor',
        width = 80, height = math.min(#lines + 2, 30),
        row = 5, col = 10,
        style = 'minimal', border = 'rounded',
        title = ' CKB Violations ', title_pos = 'center',
    })
    vim.api.nvim_buf_set_keymap(buf, 'n', 'q', ':close<CR>', { noremap = true, silent = true })
end

-- ── Impact Analysis ────────────────────────────────────────────────────────
function M.impact()
    local file = vim.fn.expand('%:p')
    local root = vim.fn.getcwd()
    local rel = file:gsub(root .. '/', '')
    local line = vim.api.nvim_win_get_cursor(0)[1]

    if not is_server_up() then
        vim.notify('[CKB] Server not running', vim.log.levels.WARN)
        return
    end

    vim.notify('[CKB] Analyzing impact for ' .. rel .. ':' .. line, vim.log.levels.INFO)

    local impact, err = api_post('/api/v1/impact', {
        path = root, file = rel, line = line, change_type = 'modify'
    })

    if not impact then
        vim.notify('[CKB] Impact analysis failed. Scan the project first.', vim.log.levels.ERROR)
        return
    end

    local risk_pct = math.floor((impact.risk_score or 0) * 100)
    local risk_icon = risk_pct >= 70 and '🔴 HIGH' or (risk_pct >= 40 and '🟡 MEDIUM' or '🟢 LOW')

    local lines = {
        string.format(' CKB Impact: %s:%d', rel, line),
        string.format(' Risk: %s (%d%%)', risk_icon, risk_pct),
        string.format(' Effort: %s', impact.estimated_effort or 'unknown'),
        '',
    }

    local direct = impact.directly_affected or {}
    if #direct > 0 then
        table.insert(lines, string.format(' Directly affected (%d):', #direct))
        for i, f in ipairs(direct) do
            if i > 8 then table.insert(lines, string.format('   ...%d more', #direct - 8)); break end
            table.insert(lines, '   • ' .. tostring(f))
        end
        table.insert(lines, '')
    end

    local transitive = impact.transitively_affected or {}
    if #transitive > 0 then
        table.insert(lines, string.format(' Transitively affected (%d):', #transitive))
        for i, f in ipairs(transitive) do
            if i > 5 then table.insert(lines, string.format('   ...%d more', #transitive - 5)); break end
            table.insert(lines, '   • ' .. tostring(f))
        end
    end

    local buf = vim.api.nvim_create_buf(false, true)
    vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
    vim.api.nvim_open_win(buf, true, {
        relative = 'editor',
        width = 70, height = math.min(#lines + 2, 25),
        row = 5, col = 10,
        style = 'minimal', border = 'rounded',
        title = ' CKB Impact Analysis ', title_pos = 'center',
    })
    vim.api.nvim_buf_set_keymap(buf, 'n', 'q', ':close<CR>', { noremap = true, silent = true })
end

-- ── Status ─────────────────────────────────────────────────────────────────
function M.status()
    local up = is_server_up()
    if not up then
        vim.notify('[CKB] Server offline. Start with: ckb serve', vim.log.levels.WARN)
        return
    end
    local report = api_get('/api/v1/report')
    if not report then
        vim.notify('[CKB] Server online. No scan yet — run :CkbScan', vim.log.levels.INFO)
        return
    end
    local v = #(report.drift or {})
    vim.notify(string.format('[CKB] ✅ Online | %d files | %d nodes | %d violations',
        report.files_processed or 0, report.nodes or 0, v), vim.log.levels.INFO)
end

-- ── Statusline component ───────────────────────────────────────────────────
function M.statusline()
    local v = #diagnostics_cache
    if v == 0 then return ' CKB ✅' end
    local crits = 0
    for _, d in ipairs(diagnostics_cache) do
        if d.severity == 'Critical' or d.severity == 'Error' then crits = crits + 1 end
    end
    return string.format(' CKB %s %d', crits > 0 and '🔴' or '🟡', v)
end

function M.update_statusline() end -- hook for custom statuslines

return M
