ilnk_proto = Proto("SnapCast", "SnapCast")
ilnk_proto.fields = {}
ilnk_proto.fields.type 		= ProtoField.uint16("SnapCast.type", "Type")
ilnk_proto.fields.id 		= ProtoField.uint16("SnapCast.id", "Id")
ilnk_proto.fields.refers_to = ProtoField.uint16("SnapCast.refers_to", "Refers to")
ilnk_proto.fields.sent_ts 	= ProtoField.uint64("SnapCast.sent_ts", "Sent timestamp")
ilnk_proto.fields.recv_ts	= ProtoField.uint64("SnapCast.recv_ts", "Received timestamp")
ilnk_proto.fields.size 		= ProtoField.uint32("SnapCast.size", "Size")
ilnk_proto.fields.wirechunk_size 		= ProtoField.uint32("SnapCast.wirechunk_size", "Wire chunk size")

function ilnk_proto.dissector(buffer, pinfo, tree)
    local packet_length = buffer:len()
	local subtree = tree:add(ilnk_proto, buffer(), "SnapCast")
	local size = buffer(22, 4)
	local type = buffer(0, 2)
	subtree:add_le(ilnk_proto.fields.type, type)
	subtree:add_le(ilnk_proto.fields.id, buffer(2, 2))
	subtree:add_le(ilnk_proto.fields.refers_to, buffer(4, 2))
	subtree:add_le(ilnk_proto.fields.sent_ts, buffer(6, 8))
	subtree:add_le(ilnk_proto.fields.recv_ts, buffer(14, 8))
	subtree:add_le(ilnk_proto.fields.size, size)

	local payload = ByteArray.tvb(buffer(26, size:le_uint()):bytes(), "Payload")

	if type:le_uint() == 2 then
		subtree:add_le(ilnk_proto.fields.wirechunk_size, buffer(34, 4))
		local wirechunk = ByteArray.tvb(buffer(38, size:le_uint()-12):bytes(), "WireChunk")
	end
end
tdp_table = DissectorTable.get("tcp.port"):add(1704, ilnk_proto)
