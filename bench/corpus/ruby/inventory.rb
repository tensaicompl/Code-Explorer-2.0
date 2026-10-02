# frozen_string_literal: true

require "json"
require_relative "dsl"

# Modules, mixins, blocks, procs, heredocs and metaprogramming.
module Corpus
  module Countable
    def count_by(&block)
      each_with_object(Hash.new(0)) { |item, acc| acc[block.call(item)] += 1 }
    end
  end

  class Inventory
    include Enumerable
    include Countable

    Item = Struct.new(:sku, :qty, keyword_init: true) do
      def to_s = "#{sku} x#{qty}"
    end

    attr_reader :items

    def initialize(limit: ENV.fetch("INVENTORY_LIMIT", 100).to_i)
      @limit = limit
      @items = {}
    end

    def add(sku, qty = 1)
      raise ArgumentError, "sku required" if sku.nil? || sku.empty?

      (@items[sku] ||= Item.new(sku: sku, qty: 0)).qty += qty
      self
    end

    def each(&block)
      return enum_for(:each) unless block_given?

      @items.each_value(&block)
    end

    def report
      <<~TEXT
        Inventory: #{@items.size} skus
        Largest: #{max_by(&:qty)&.sku || "none"}
        Units: #{sum(&:qty)}
      TEXT
    end

    def method_missing(name, *args, &block)
      if name.to_s.start_with?("find_")
        @items[name.to_s.delete_prefix("find_")]
      else
        super
      end
    end

    def respond_to_missing?(name, include_private = false)
      name.to_s.start_with?("find_") || super
    end

    def to_json(*opts) = { items: @items.transform_values(&:qty) }.to_json(*opts)
  end
end

doubler = ->(x) { x * 2 }
squares = (1..5).map { |n| n**2 }.select(&:even?)
inventory = Corpus::Inventory.new(limit: 10).add("café", 2).add("thé")
puts inventory.report, doubler.(squares.sum), inventory.count_by { |i| i.qty.odd? }
