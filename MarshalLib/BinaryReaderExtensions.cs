using System.Buffers.Binary;
using System.Text;

namespace MarshalLib;

// https://github.com/dotnet/runtime/issues/26904
internal static class BinaryReaderExtensions
{
    public static string ReadCString(this BinaryReader binaryReader)
    {
        var bytes = new List<byte>(16);
        byte b;
        while ((b = binaryReader.ReadByte()) != 0)
        {
            bytes.Add(b);
        }

        return Encoding.UTF8.GetString([.. bytes]);
    }

    public static short ReadInt16BigEndian(this BinaryReader binaryReader) => BinaryPrimitives.ReadInt16BigEndian(
        binaryReader.ReadSpan(stackalloc byte[2]));

    public static ushort ReadUInt16BigEndian(this BinaryReader binaryReader) => BinaryPrimitives.ReadUInt16BigEndian(
        binaryReader.ReadSpan(stackalloc byte[2]));

    public static int ReadInt32BigEndian(this BinaryReader binaryReader) => BinaryPrimitives.ReadInt32BigEndian(
        binaryReader.ReadSpan(stackalloc byte[4]));

    public static uint ReadUInt32BigEndian(this BinaryReader binaryReader) => BinaryPrimitives.ReadUInt32BigEndian(
        binaryReader.ReadSpan(stackalloc byte[4]));

    public static long ReadInt64BigEndian(this BinaryReader binaryReader) => BinaryPrimitives.ReadInt64BigEndian(
        binaryReader.ReadSpan(stackalloc byte[8]));

    public static ulong ReadUInt64BigEndian(this BinaryReader binaryReader) => BinaryPrimitives.ReadUInt64BigEndian(
        binaryReader.ReadSpan(stackalloc byte[8]));

    public static Half ReadHalfBigEndian(this BinaryReader binaryReader) => BinaryPrimitives.ReadHalfBigEndian(
        binaryReader.ReadSpan(stackalloc byte[2]));

    public static float ReadSingleBigEndian(this BinaryReader binaryReader) => BinaryPrimitives.ReadSingleBigEndian(
        binaryReader.ReadSpan(stackalloc byte[4]));

    public static double ReadDoubleBigEndian(this BinaryReader binaryReader) => BinaryPrimitives.ReadDoubleBigEndian(
        binaryReader.ReadSpan(stackalloc byte[8]));

    private static ReadOnlySpan<byte> ReadSpan(this BinaryReader binaryReader, Span<byte> buffer)
    {
        binaryReader.BaseStream.ReadExactly(buffer);
        return buffer;
    }
}