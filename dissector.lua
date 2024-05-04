ilnk_proto = Proto("SnapCast", "SnapCast")
ilnk_proto.fields = {}
-- ilnk_proto.fields.type 				= ProtoField.uint16("SnapCast.type", "Type")
ilnk_proto.fields.type 					= ProtoField.string("SnapCast.type", "Type")
ilnk_proto.fields.id 					= ProtoField.uint16("SnapCast.id", "Id")
ilnk_proto.fields.refers_to 			= ProtoField.uint16("SnapCast.refers_to", "Refers to")
ilnk_proto.fields.sent_ts_s				= ProtoField.int32("SnapCast.sent_ts_s", "Sent timestamp(s)")
ilnk_proto.fields.sent_ts_us			= ProtoField.int32("SnapCast.sent_ts_us", "Sent timestamp(us)")
ilnk_proto.fields.recv_ts_s				= ProtoField.int32("SnapCast.recv_ts_s", "Received timestamp(s)")
ilnk_proto.fields.recv_ts_us			= ProtoField.int32("SnapCast.recv_ts_us", "Received timestamp(us)")
ilnk_proto.fields.size 					= ProtoField.uint32("SnapCast.size", "Size")

-- type 1 (CodecHeader)
ilnk_proto.fields.codec_name 			= ProtoField.string("SnapCast.codec_name", "Codec Name")

-- type 2 (Wirechunk)
ilnk_proto.fields.wirechunk_size 		= ProtoField.uint32("SnapCast.wirechunk_size", "Wire chunk size")
ilnk_proto.fields.play_at_s				= ProtoField.uint32("SnapCast.play_at_s", "Playback timestamp (s)")
ilnk_proto.fields.play_at_us			= ProtoField.uint32("SnapCast.play_at_us", "Playback timestamp (us)")
ilnk_proto.fields.payload				= ProtoField.bytes("SnapCast.payload", "Payload")

-- type 3 (ServerSettings)
ilnk_proto.fields.server_settings		= ProtoField.string("SnapCast.server_settings", "Server Settings")

-- type 4 (Time)
ilnk_proto.fields.latency_sec 			= ProtoField.int32("SnapCast.latency_sec", "Latency sec")
ilnk_proto.fields.latency_usec 			= ProtoField.int32("SnapCast.latency_usec", "Latency usec")

-- type 5 (Hello)
ilnk_proto.fields.client_hello 			= ProtoField.string("SnapCast.client_hello", "Client Hello")

partial_bufs = {}
pending_bytes = 0

function ilnk_proto.init()
	pending_bytes = 0
	partial_bufs = {}
end

function ilnk_proto.dissector(buffer, pinfo, tree)
    local packet_length = buffer:len()
	local subtree = tree:add(ilnk_proto, buffer(), "SnapCast")
	local lut = {
		[0] = "Base",
		[1] = "CodecHeader",
		[2] = "WireChunk",
		[3] = "ServerSettings",
		[4] = "Time",
		[5] = "Hello",
		[6] = "StreamTags",
		[7] = "ClientInfo",
	}

	local type = buffer(0, 2):le_uint()
	local typename = lut[type]
	-- what's the likelihood of a data packet having a legal header??
	if lut[type] == nil then
		return 0
		--if pending_bytes > 0 then
		--	local payload_in_packet = buffer:len()
		--	pinfo.desegment_offset = payload_in_packet
		--	pending_bytes = pending_bytes - payload_in_packet
		--	pinfo.desegment_len = pending_bytes
		--	print("pending bytes ".. pending_bytes)
		--	if pending_bytes == 0 then
		--		typename = "ReassembledWireChunk"
		--		print("reassembly done")
		--	else
		--		partial_bufs[#partial_bufs+1] = buffer()
		--		return payload_in_packet
		--	end
		--else
		--	print("rejecting packet " .. buffer(0, 4))
		--	-- bad packet?
		--	return 0
		--end
	end

	local size = buffer(22, 4)
	pinfo.cols.info:set("SnapCast " .. typename)
	subtree:add(ilnk_proto.fields.type, typename)

	if typename ~= "ReassembledWireChunk" then
		subtree:add_le(ilnk_proto.fields.id, buffer(2, 2))
		subtree:add_le(ilnk_proto.fields.refers_to, buffer(4, 2))
		subtree:add_le(ilnk_proto.fields.sent_ts_s, buffer(6, 4))
		subtree:add_le(ilnk_proto.fields.sent_ts_us, buffer(10, 4))
		subtree:add_le(ilnk_proto.fields.recv_ts_s, buffer(14, 4))
		subtree:add_le(ilnk_proto.fields.recv_ts_us, buffer(18, 4))
		subtree:add_le(ilnk_proto.fields.size, size)
	end

	if typename == "CodecHeader" then
		local strsize = buffer(26, 4):le_uint()
		subtree:add_le(ilnk_proto.fields.codec_name, buffer(30, strsize))
	elseif typename == "Hello" then
		local strsize = buffer(26, 4):le_uint()
		subtree:add_le(ilnk_proto.fields.client_hello, buffer(30, strsize))
	elseif typename == "ServerSettings" then
		local strsize = buffer(26, 4):le_uint()
		subtree:add_le(ilnk_proto.fields.server_settings, buffer(30, strsize))
	elseif typename == "WireChunk" then

		subtree:add_le(ilnk_proto.fields.play_at_s, buffer(26, 4):le_int())
		subtree:add_le(ilnk_proto.fields.play_at_us, buffer(30, 4):le_int())

		local wc_size = buffer(34, 4)
		subtree:add_le(ilnk_proto.fields.wirechunk_size, wc_size)

		local analyzed_bytes = buffer:len()
		pinfo.desegment_offset = analyzed_bytes
		local header_len = 38
		local payload_in_packet = buffer:len() - header_len
		pending_bytes = wc_size:le_uint() - payload_in_packet
		pinfo.desegment_len = pending_bytes
		partial_bufs = {}
		partial_bufs[#partial_bufs+1] = buffer(38, payload_in_packet)

		-- return analyzed_bytes
		-- TODO: reassembly of packets, as a chunk spans multiple
		-- https://ask.wireshark.org/question/11650/lua-wireshark-dissector-combine-data-from-2-udp-packets/
		--local wirechunk = ByteArray.tvb(buffer(38, wc_size:le_uint()-38):bytes(), "WireChunk")
	elseif typename == "ReassembledWireChunk" then
		print("process")
		local concat_payload = ByteArray.new()
		for i, v in ipairs(partial_bufs) do
			concat_payload:append(v)
		end
		local tvb = ByteArray.tvb(concat_payload, "Reassembled Payload")
		subtree:add(ilnk_proto.fields.payload, tvb) -- :set_generated()
		partial_bufs = {}
	elseif typename == "Time" then
		subtree:add_le(ilnk_proto.fields.latency_sec, buffer(26, 4))
		subtree:add_le(ilnk_proto.fields.latency_sec, buffer(30, 4))
	end

end
tdp_table = DissectorTable.get("tcp.port"):add(1704, ilnk_proto)
