class Pipeline
  def self.define(&block)
    new.tap { |p| p.instance_eval(&block) }
  end

  def initialize
    @steps = []
  end

  def step(name, **opts, &block)
    @steps << { name: name, opts: opts, run: block }
  end

  def run(input)
    @steps.reduce(input) do |acc, s|
      s[:run].call(acc)
    rescue StandardError => e
      warn "step #{s[:name]} failed: #{e.message}"
      acc
    end
  end
end

PIPELINE = Pipeline.define do
  step :strip, retries: 1 do |s|
    s.strip
  end
  step(:upcase) { |s| s.upcase }
  step :wrap do |s|
    case s
    when /\A\d+\z/ then "number:#{s}"
    when String then %Q("#{s}")
    else s.to_s
    end
  end
end
