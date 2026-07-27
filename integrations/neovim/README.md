# CKB for Neovim — Setup Guide

## Requirements
- Neovim 0.9+
- `plenary.nvim`
- `curl` in PATH

## Install

### With lazy.nvim
```lua
{
    -- Copy ckb.lua to your config, or use as a plugin
    'ckbdev/ckb.nvim',  -- (not on GitHub yet — use local for now)
    dependencies = { 'nvim-lua/plenary.nvim' },
    config = function()
        require('ckb').setup({
            server_url = 'http://localhost:3000',
            auto_scan_on_open = true,
            show_diagnostics = true,
        })
    end
}
```

### Manual (copy file)
```bash
mkdir -p ~/.config/nvim/lua
cp integrations/neovim/lua/ckb.lua ~/.config/nvim/lua/ckb.lua
```

Add to `init.lua`:
```lua
require('ckb').setup()
```

## Commands

| Command | Keymap | Description |
|---------|--------|-------------|
| `:CkbScan` | `<leader>cs` | Full project scan |
| `:CkbCheck` | `<leader>cc` | Check architecture (floating window) |
| `:CkbImpact` | `<leader>ci` | Impact analysis at cursor |
| `:CkbStatus` | `<leader>cx` | Server + scan status |

## Statusline Integration

```lua
-- lualine example:
require('lualine').setup {
    sections = {
        lualine_x = { require('ckb').statusline },
    }
}
```

## Diagnostics
Violations appear as errors/warnings in your files via `vim.diagnostic`.
View them with `:Telescope diagnostics` or `:lua vim.diagnostic.open_float()`.
