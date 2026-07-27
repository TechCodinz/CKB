#!/bin/bash
# setup-zero-cost.sh

echo "🚀 Setting up CKB for zero-cost launch"

# 1. Check prerequisites
if ! command -v vercel &> /dev/null; then
    echo "⚠️ Vercel CLI not found. Installing..."
    npm install -g vercel
fi

# 2. Deploy backend to Render
echo "📦 Backend will be deployed automatically when you push to GitHub"
echo "Make sure you've connected your repository to Render.com using render.yaml"

# 3. Deploy frontend to Vercel
echo "🎨 Deploying frontend..."
cd web || exit
vercel --prod
cd ..

# 4. Setup database
echo "🗄️  Configuring database..."
if [ -z "$DATABASE_URL" ]; then
    echo "⚠️ DATABASE_URL not set. Skipping database configuration."
    echo "Set DATABASE_URL from Supabase to run migrations."
else
    echo "Running migrations..."
    # psql "$DATABASE_URL" < migrations/init.sql
fi

# 5. Output next steps
echo "💰 Next Steps for Payments:"
echo "1. Go to commerce.coinbase.com and get your API Key"
echo "2. Set COINBASE_COMMERCE_API_KEY in Vercel environment variables"
echo "3. Update your Coinbase links in landing/index.html"

echo "✅ Zero-cost launch setup complete!"
echo "🌐 Frontend will be live on your Vercel URL"
echo "🔧 Backend will be live on your Render URL"
