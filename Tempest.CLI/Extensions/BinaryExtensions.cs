namespace Tempest.CLI.Extensions;

internal static class BinaryExtensions
{
    public static long IndexOfBytes(this Stream stream, byte[] bytes)
    {
        var currentByte = 0;
        var position = stream.Position;
        var headerMatchCount = 0;

        while ((currentByte = stream.ReadByte()) != -1)
        {
            if (currentByte == bytes[headerMatchCount])
            {
                headerMatchCount++;
                if (headerMatchCount == bytes.Length)
                {
                    return position - bytes.Length + 1;
                }
            }
            else
            {
                headerMatchCount = 0;
                if (currentByte == bytes[0])
                {
                    headerMatchCount = 1;
                }
            }
            position++;
        }

        return -1;
    }

    public static long IndexOfBytes(this byte[] source, byte[] bytes)
    {
        var position = 0;
        var headerMatchCount = 0;

        while (position < source.Length)
        {
            var currentByte = source[position];

            if (currentByte == bytes[headerMatchCount])
            {
                headerMatchCount++;
                if (headerMatchCount == bytes.Length)
                {
                    return position - bytes.Length + 1;
                }
            }
            else
            {
                headerMatchCount = 0;
                if (currentByte == bytes[0])
                {
                    headerMatchCount = 1;
                }
            }
            position++;
        }

        return -1;
    }
}