with Ada.Containers.Ordered_Maps;
with Ada.Strings.Unbounded; use Ada.Strings.Unbounded;

package Inventory is

   type Quantity is range 0 .. 1_000_000;

   type Status is (Pending, Done, Failed);

   type Item is record
      Sku   : Unbounded_String;
      Count : Quantity := 0;
      State : Status := Pending;
   end record;

   package Item_Maps is new Ada.Containers.Ordered_Maps
     (Key_Type     => Unbounded_String,
      Element_Type => Item);

   type Store is tagged private;

   procedure Add (S : in out Store; Sku : String; Count : Quantity);
   function Total (S : Store) return Natural;

   generic
      type Element is private;
      with function "<" (L, R : Element) return Boolean is <>;
   function Max_Of (A, B : Element) return Element;

private

   type Store is tagged record
      Items : Item_Maps.Map;
   end record;

end Inventory;
