using System.Buffers.Binary;

namespace Tempest.CLI.Marshal;

/// <summary>
/// Reads a named C++ enum out of the DWARF debug info of an ELF64 little-endian
/// binary. The game's server-side <c>MFIELD_TOKEN</c> enum (the authoritative
/// field numbering for .dat files) is only present as debug info in the Linux
/// builds (<c>libMctsInterface.so</c>, <c>ShippingPC-ChaosServer</c>); the
/// Windows binaries are stripped of it.
/// </summary>
internal static class DwarfEnumReader
{
    private sealed class Section
    {
        public int NameOffset;
        public int Offset;
        public int Size;
        public byte[] Bytes = [];
    }

    private sealed record Abbrev(uint Tag, bool HasChildren, List<(uint Attr, uint Form)> Attributes);

    /// <summary>Returns (value, name) pairs for the named enum, or null when the file has no usable DWARF.</summary>
    public static (int Value, string Name)[]? TryReadEnum(byte[] file, string target)
    {
        if (file.Length < 0x40
            || file[0] != 0x7F || file[1] != (byte)'E' || file[2] != (byte)'L' || file[3] != (byte)'F'
            || file[4] != 2 /* ELF64 */ || file[5] != 1 /* little-endian */)
        {
            return null;
        }

        var sectionHeaderOffset = BinaryPrimitives.ReadUInt64LittleEndian(file.AsSpan(0x28));
        var sectionHeaderSize = BinaryPrimitives.ReadUInt16LittleEndian(file.AsSpan(0x3A));
        var sectionCount = BinaryPrimitives.ReadUInt16LittleEndian(file.AsSpan(0x3C));
        var stringTableIndex = BinaryPrimitives.ReadUInt16LittleEndian(file.AsSpan(0x3E));

        if (sectionHeaderSize < 0x40 || stringTableIndex >= sectionCount)
            return null;

        var sections = new List<Section>(sectionCount);

        for (var i = 0; i < sectionCount; i++)
        {
            var offset = checked((int)(sectionHeaderOffset + (ulong)i * sectionHeaderSize));
            if (offset + 0x40 > file.Length)
                return null;

            var header = file.AsSpan(offset);
            var section = new Section
            {
                NameOffset = BinaryPrimitives.ReadInt32LittleEndian(header[0x00..]),
                Offset = checked((int)BinaryPrimitives.ReadUInt64LittleEndian(header[0x18..])),
                Size = checked((int)BinaryPrimitives.ReadUInt64LittleEndian(header[0x20..])),
            };

            if (section.Offset < 0 || section.Size < 0 || section.Offset + section.Size > file.Length)
                return null;

            section.Bytes = file.AsSpan(section.Offset, section.Size).ToArray();
            sections.Add(section);
        }

        var stringTable = sections[stringTableIndex];
        var info = FindSection(sections, stringTable.Bytes, ".debug_info");
        var abbrev = FindSection(sections, stringTable.Bytes, ".debug_abbrev");
        var str = FindSection(sections, stringTable.Bytes, ".debug_str");

        if (info is null || abbrev is null || str is null)
            return null;

        var infoData = info.Bytes;
        var pos = 0;

        while (pos + 11 <= infoData.Length)
        {
            var unitLength = BinaryPrimitives.ReadUInt32LittleEndian(infoData.AsSpan(pos));
            var version = BinaryPrimitives.ReadUInt16LittleEndian(infoData.AsSpan(pos + 4));
            var abbrevOffset = BinaryPrimitives.ReadUInt32LittleEndian(infoData.AsSpan(pos + 6));
            var addressSize = infoData[pos + 10];

            // DWARF 2-4 only; v5 changes the header layout and string forms.
            if (version is < 2 or > 4)
                return null;

            var unitEnd = checked(pos + 4 + (int)unitLength);
            if (unitEnd > infoData.Length)
                return null;

            var abbrevTable = ReadAbbrevTable(abbrev.Bytes, checked((int)abbrevOffset));
            if (abbrevTable is null)
                return null;

            var found = Walk(infoData, pos + 11, unitEnd, abbrevTable, addressSize, str.Bytes, target, out var entries);
            if (entries != null)
                return entries;

            pos = found;
        }

        return null;
    }

    /// <summary>
    /// Walks the DIE sequence, consuming it up to the end-of-siblings marker
    /// (abbrev 0) or the sequence end. Returns the position after the consumed
    /// sequence; sets <paramref name="found"/> when the target enum was parsed.
    /// </summary>
    private static int Walk(
        byte[] info, int pos, int end, Dictionary<uint, Abbrev> abbrevTable, int addressSize, byte[] strData, string target,
        out (int Value, string Name)[]? found)
    {
        found = null;

        while (pos < end)
        {
            if (!TryReadUleb(info, ref pos, out var code))
                return end;

            if (code == 0)
                return pos; // end of siblings

            if (!abbrevTable.TryGetValue(code, out var abbrev))
                return end;

            var after = SkipAttributes(info, pos, abbrev, addressSize);
            if (after < 0)
                return end;

            if (abbrev.Tag == 0x04) // DW_TAG_enumeration_type
            {
                if (ReadName(info, pos, abbrev, addressSize, strData) == target)
                {
                    found = ParseEnumerators(info, after, end, abbrevTable, addressSize, strData);
                    return end;
                }
            }

            if (abbrev.HasChildren)
            {
                var childPos = Walk(info, after, end, abbrevTable, addressSize, strData, target, out var childFound);
                if (childFound != null)
                {
                    found = childFound;
                    return end;
                }

                pos = childPos;
            }
            else
            {
                pos = after;
            }
        }

        return pos;
    }

    private static (int Value, string Name)[]? ParseEnumerators(
        byte[] info, int start, int end, Dictionary<uint, Abbrev> abbrevTable, int addressSize, byte[] strData)
    {
        var entries = new List<(int, string)>();
        var childPos = start;

        while (childPos < end)
        {
            if (!TryReadUleb(info, ref childPos, out var childCode))
                return null;

            if (childCode == 0)
                break;

            if (!abbrevTable.TryGetValue(childCode, out var childAbbrev) || childAbbrev.Tag != 0x28)
                return null; // DW_TAG_enumerator

            var enumeratorName = ReadName(info, childPos, childAbbrev, addressSize, strData);
            var attrPos = childPos;
            int? enumeratorValue = null;

            foreach (var (attr, form) in childAbbrev.Attributes)
            {
                if (attr == 0x1C) // DW_AT_const_value
                {
                    enumeratorValue = form switch
                    {
                        0x0b => info[attrPos],                                                                       // data1
                        0x05 => BinaryPrimitives.ReadUInt16LittleEndian(info.AsSpan(attrPos)),                      // data2
                        0x06 => unchecked((int)BinaryPrimitives.ReadUInt32LittleEndian(info.AsSpan(attrPos))),      // data4
                        0x07 => unchecked((int)BinaryPrimitives.ReadUInt64LittleEndian(info.AsSpan(attrPos))),      // data8
                        0x0d => ReadSleb(info, ref attrPos),                                                        // sdata
                        0x0f => unchecked((int)ReadUlebValue(info, ref attrPos)),                                   // udata
                        _ => null
                    };
                }

                attrPos = SkipForm(info, attrPos, form, addressSize);
                if (attrPos < 0)
                    return null;
            }

            if (enumeratorName is null || enumeratorValue is null)
                return null;

            entries.Add((enumeratorValue.Value, enumeratorName));
            childPos = attrPos;
        }

        return entries.ToArray();
    }

    private static Dictionary<uint, Abbrev>? ReadAbbrevTable(byte[] abbrev, int offset)
    {
        var table = new Dictionary<uint, Abbrev>();
        var pos = offset;

        while (true)
        {
            if (!TryReadUleb(abbrev, ref pos, out var code))
                return null;

            if (code == 0)
                break;

            if (!TryReadUleb(abbrev, ref pos, out var tag) || pos >= abbrev.Length)
                return null;

            var hasChildren = abbrev[pos++] != 0;
            var attributes = new List<(uint Attr, uint Form)>();

            while (true)
            {
                if (!TryReadUleb(abbrev, ref pos, out var attr) || !TryReadUleb(abbrev, ref pos, out var form))
                    return null;

                if (attr == 0 && form == 0)
                    break;

                attributes.Add((attr, form));
            }

            table[code] = new Abbrev(tag, hasChildren, attributes);
        }

        return table;
    }

    private static int SkipAttributes(byte[] info, int pos, Abbrev abbrev, int addressSize)
    {
        foreach (var (_, form) in abbrev.Attributes)
        {
            pos = SkipForm(info, pos, form, addressSize);
            if (pos < 0)
                return -1;
        }

        return pos;
    }

    private static int SkipForm(byte[] info, int pos, uint form, int addressSize)
    {
        switch (form)
        {
            case 0x01: return pos + addressSize;                              // addr
            case 0x03: return pos + 2 + ReadU16(info, pos);                   // block2
            case 0x04: return pos + 4 + (int)ReadU32(info, pos);              // block4
            case 0x05: return pos + 2;                                        // data2
            case 0x06: return pos + 4;                                        // data4
            case 0x07: return pos + 8;                                        // data8
            case 0x08:                                                         // string
                {
                    var end = Array.IndexOf(info, (byte)0, pos);
                    return end < 0 ? -1 : end + 1;
                }
            case 0x09: return pos + (int)ReadUlebValue(info, ref pos);        // block
            case 0x0a: return pos + 1 + info[pos];                            // block1
            case 0x0b: return pos + 1;                                        // data1
            case 0x0c: return pos + 1;                                        // flag
            case 0x0d: ReadSleb(info, ref pos); return pos;                   // sdata
            case 0x0e: return pos + 4;                                        // strp
            case 0x0f: ReadUlebValue(info, ref pos); return pos;              // udata
            case 0x10: return pos + addressSize;                              // ref_addr
            case 0x11: return pos + 1;                                        // ref1
            case 0x12: return pos + 2;                                        // ref2
            case 0x13: return pos + 4;                                        // ref4
            case 0x14: return pos + 8;                                        // ref8
            case 0x15: ReadUlebValue(info, ref pos); return pos;              // ref_udata
            case 0x16:                                                         // indirect
                if (!TryReadUleb(info, ref pos, out var inner))
                    return -1;
                return SkipForm(info, pos, inner, addressSize);
            case 0x17: return pos + 4;                                        // sec_offset
            case 0x18: return pos + (int)ReadUlebValue(info, ref pos);        // exprloc
            case 0x19: return pos;                                            // flag_present
            default: return -1;                                               // unsupported
        }
    }

    private static string? ReadName(byte[] info, int pos, Abbrev abbrev, int addressSize, byte[] strData)
    {
        foreach (var (attr, form) in abbrev.Attributes)
        {
            if (attr == 0x03) // DW_AT_name
            {
                return form switch
                {
                    0x0e => ReadStrp(info, pos, strData),
                    0x08 => ReadInlineString(info, pos),
                    _ => null
                };
            }

            pos = SkipForm(info, pos, form, addressSize);
            if (pos < 0)
                return null;
        }

        return null;
    }

    private static string? ReadStrp(byte[] info, int pos, byte[] strData)
    {
        var offset = ReadU32(info, pos);
        if (offset >= strData.Length)
            return null;

        var end = Array.IndexOf(strData, (byte)0, (int)offset);
        if (end < 0)
            return null;

        return System.Text.Encoding.UTF8.GetString(strData, (int)offset, end - (int)offset);
    }

    private static string? ReadInlineString(byte[] info, int pos)
    {
        if (pos >= info.Length)
            return null;

        var end = Array.IndexOf(info, (byte)0, pos);
        if (end < 0)
            return null;

        return System.Text.Encoding.UTF8.GetString(info, pos, end - pos);
    }

    private static bool TryReadUleb(byte[] buffer, ref int pos, out uint value)
    {
        ulong acc = 0;
        var shift = 0;

        while (pos < buffer.Length)
        {
            var b = buffer[pos++];

            // 64-bit values (e.g. udata constants) only need their byte count
            // consumed; the truncated value is irrelevant for skipping.
            if (shift < 64)
                acc |= (ulong)(b & 0x7F) << shift;

            if ((b & 0x80) == 0)
            {
                value = (uint)acc;
                return true;
            }

            shift += 7;
            if (shift >= 70)
                break;
        }

        value = (uint)acc;
        return false;
    }

    private static uint ReadUlebValue(byte[] buffer, ref int pos) =>
        TryReadUleb(buffer, ref pos, out var value) ? value : 0;

    private static int ReadSleb(byte[] buffer, ref int pos)
    {
        long value = 0;
        var shift = 0;

        while (pos < buffer.Length)
        {
            var b = buffer[pos++];

            // 64-bit values need all bytes consumed for alignment; the sign
            // extension below only matters for the (rarely used) value itself.
            if (shift < 63)
                value |= (long)(b & 0x7F) << shift;

            if ((b & 0x80) == 0)
            {
                if (shift < 63 && (b & 0x40) != 0)
                    value |= -(1L << shift);
                return (int)value;
            }

            shift += 7;
            if (shift >= 70)
                break;
        }

        return (int)value;
    }

    private static ushort ReadU16(byte[] info, int pos) =>
        pos + 2 <= info.Length ? BinaryPrimitives.ReadUInt16LittleEndian(info.AsSpan(pos)) : (ushort)0;

    private static uint ReadU32(byte[] info, int pos) =>
        pos + 4 <= info.Length ? BinaryPrimitives.ReadUInt32LittleEndian(info.AsSpan(pos)) : 0;

    private static Section? FindSection(List<Section> sections, byte[] stringTable, string name)
    {
        var nameBytes = System.Text.Encoding.UTF8.GetBytes(name);

        foreach (var section in sections)
        {
            if (section.NameOffset < 0 || section.NameOffset >= stringTable.Length)
                continue;

            if (stringTable.AsSpan(section.NameOffset).StartsWith(nameBytes)
                && section.NameOffset + nameBytes.Length < stringTable.Length
                && stringTable[section.NameOffset + nameBytes.Length] == 0)
            {
                return section;
            }
        }

        return null;
    }
}
