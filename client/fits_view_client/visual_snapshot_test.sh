#!/bin/bash
# Visual snapshot test script for FITS viewer

echo "🚀 Starting visual snapshot test..."

# Kill any existing processes
pkill -f simple_test_server || true
pkill -f fits_view_client || true
sleep 1

# Start the simple test server
echo "📡 Starting simple test server..."
cd ../../../server
python simple_test_server.py &
SERVER_PID=$!
sleep 3

# Test server is running
curl -s http://127.0.0.1:8002/debug/pattern > /dev/null
if [ $? -eq 0 ]; then
    echo "✅ Server is running"
else
    echo "❌ Server failed to start"
    kill $SERVER_PID 2>/dev/null
    exit 1
fi

# Start client with specific pan settings for upper right corner
echo "🖥️  Starting client with upper right corner positioning..."
cd ../client/fits_view_client

# We need to modify the client to accept pan parameters or create a test version
echo "📝 Note: Client needs modification to accept initial pan offset"
echo "   Expected pan offset: [-256, 256] to center upper right corner"

# For now, document the manual test procedure
echo ""
echo "📋 Manual Test Procedure:"
echo "1. Run: cargo run -- --backend-url http://127.0.0.1:8002"
echo "2. Drag the image so upper right corner is at window center"
echo "3. Expected result: Light gray (200) should dominate center"
echo "4. Take screenshot and compare with expected pattern"
echo ""

# Cleanup
echo "🧹 Cleaning up..."
kill $SERVER_PID 2>/dev/null
echo "✅ Snapshot test script created"
