-- PlxNative owns all transport decisions and draws every control.
local bindings = {LEFT='left', RIGHT='right', UP='up', DOWN='down', ENTER='ok', ESC='back', BS='back', SPACE='playpause', MBTN_RIGHT='back'}
for key, token in pairs(bindings) do
    mp.add_forced_key_binding(key, 'plx-' .. key, function()
        mp.commandv('script-message', 'plx-key', token)
    end, {repeatable=true})
end
mp.add_forced_key_binding('MBTN_LEFT', 'plx-pointer', function()
    local pos = mp.get_property_native('mouse-pos')
    if pos then mp.commandv('script-message', 'plx-click', tostring(pos.x), tostring(pos.y)) end
end)
