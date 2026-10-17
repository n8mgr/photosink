using Microsoft.UI.Dispatching;
using System;
using uniffi.photosink_bridge;

namespace Photosink;

internal sealed class CoreListener : Listener
{
    private readonly DispatcherQueue queue;
    private readonly Action<Change> handler;

    internal CoreListener(DispatcherQueue queue, Action<Change> handler)
    {
        this.queue = queue;
        this.handler = handler;
    }

    public void Changed(Change change) => queue.TryEnqueue(() => handler(change));
}
