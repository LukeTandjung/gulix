local previous = nil

local function is_gulix(cx, pane)
	if pane == nil then
		return false
	end
	local info = cx.session:pane(pane)
	if info == nil or info.exited ~= nil then
		return false
	end
	local tree = cx.session:surface(pane)
	return tree ~= nil
		and tree.surface ~= nil
		and tree.surface.role == "gulix.typst"
end

local poll_focus

poll_focus = function(cx)
	if cx == nil then
		return
	end
	local pane = cx.session:focused()
	if previous ~= nil and previous ~= pane and is_gulix(cx, previous) then
		cx.session:event(previous, { ev = "blur" })
	end
	previous = pane
	tern.timer(50, poll_focus)
end

tern.on("window_start", function(cx)
	previous = cx.session:focused()
	-- Older Gulix plugins removed Tern's normal horizontal inset. Clear that
	-- override and leave the pane's padding alone from now on.
	tern.css("layout", "")
	tern.timer(50, poll_focus)
end)
