using System.Threading;

namespace Tempest.CLI.Server;

internal sealed class Channel<T>
{
    private readonly ChannelWriter _writer;
    private readonly ChannelReader _reader;

    public Channel(int capacity)
    {
        var queue = new System.Collections.Concurrent.BlockingCollection<T>(capacity);
        _writer = new ChannelWriter(queue);
        _reader = new ChannelReader(queue);
    }

    public ChannelWriter Writer => _writer;
    public ChannelReader Reader => _reader;

    internal sealed class ChannelWriter
    {
        private readonly System.Collections.Concurrent.BlockingCollection<T> _queue;
        public ChannelWriter(System.Collections.Concurrent.BlockingCollection<T> queue) => _queue = queue;
        public bool TryWrite(T item)
        {
            return _queue.TryAdd(item);
        }
    }

    internal sealed class ChannelReader
    {
        private readonly System.Collections.Concurrent.BlockingCollection<T> _queue;
        public ChannelReader(System.Collections.Concurrent.BlockingCollection<T> queue) => _queue = queue;
        public bool TryRead(out T item)
        {
            return _queue.TryTake(out item!);
        }

        public async Task<bool> WaitToReadAsync(CancellationToken token)
        {
            await Task.Delay(50, token); // simple polling fallback to avoid external dependency
            return !_queue.IsCompleted && _queue.Count > 0;
        }
    }
}
